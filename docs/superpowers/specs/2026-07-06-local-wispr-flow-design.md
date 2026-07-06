# Local Wispr Flow — návrh (design doc)

**Dátum:** 2026-07-06
**Stav:** schválený užívateľom (brainstorming ukončený)
**Cieľové platformy:** macOS + Windows

## 1. Čo staviame a prečo

Desktopová diktovacia appka v štýle Wispr Flow, bez limitov free verzie
originálu. Užívateľ podrží klávesu, hovorí (slovensky, česky alebo anglicky),
pustí klávesu — a vyčistený text sa vloží na pozíciu kurzora do akejkoľvek
appky (Slack, mail, IDE…). Beží na pozadí celý pracovný deň, preto musí byť
extrémne ľahká.

**Zámerne NIE lokálny STT model** — užívateľ nechce žiadny ML model na svojom
stroji. Prepis robí Groq API (free tier), čistenie textu Claude cez Meridian
(užívateľovo Claude predplatné cez lokálnu proxy). Overené fakty:

- Anthropic API neprijíma audio — Claude nemôže robiť samotný prepis.
- Meridian (github.com/rynfar/meridian) vystavuje Claude predplatné ako
  Anthropic-kompatibilné API na `http://127.0.0.1:3456` (`/v1/messages`).
- Groq hostuje `whisper-large-v3-turbo` s free tierom, auto-detekciou jazyka
  a prepisom pod 1 s.

## 2. Kľúčové rozhodnutia (schválené užívateľom)

| Rozhodnutie | Voľba |
|---|---|
| Výstup textu | Priamo do aktívnej appky na pozíciu kurzora |
| Hotkey | Podržanie = push-to-talk; dvojité ťuknutie = toggle start/stop |
| Jazyky | Auto-detekcia SK/CS/EN + manuálny override v nastaveniach/tray |
| Feedback v bubline | Waveform podľa hlasitosti + živý čiastočný prepis |
| STT | Groq API `whisper-large-v3-turbo` (free tier, BYO kľúč) |
| Čistenie textu | Claude cez Meridian (localhost proxy), voliteľné, s fallbackom |
| História | Lokálna (SQLite), s mazaním |
| Stack | Tauri v2 — Rust backend + React/TypeScript UI |
| Distribúcia | GitHub Releases (.dmg + .msi), bez podpisovania |

## 3. Architektúra

```
┌────────────────────────── Tauri App ──────────────────────────┐
│  Rust core                                                    │
│  ├─ audio     : cpal — capture mikrofónu, RMS amplitúda       │
│  ├─ hotkey    : rdev — globálne key-down/key-up, double-tap   │
│  ├─ stt       : Groq klient (reqwest) — chunky + finálny prepis│
│  ├─ cleanup   : Meridian klient (Anthropic /v1/messages)      │
│  ├─ inject    : enigo — clipboard swap + Cmd/Ctrl+V           │
│  ├─ store     : SQLite (história) + JSON (nastavenia)         │
│  └─ state     : Idle → Recording → Transcribing → Cleaning    │
│                 → Injecting → Idle                            │
│                                                               │
│  Webview okná (React + TS, Vite)                              │
│  ├─ Bublina    : always-on-top, nekradne fokus                │
│  ├─ Hlavné okno: Wizard / Nastavenia / História               │
│  └─ Tray       : jazyk, otvorenie appky, quit                 │
└───────────────────────────────────────────────────────────────┘
   Externé: Groq API (STT) · Meridian na 127.0.0.1:3456 (cleanup)
```

Dôležité architektonické rozhodnutie: **mikrofón sa zachytáva v Ruste
(cpal), nie vo webview** — obchádza to nespoľahlivosť getUserMedia vo
WKWebView na macOS a dáva jednotné správanie na oboch OS. Webview dostáva
len eventy (amplitúda, čiastočný text, stav).

### Kritické systémové integrácie

| Potreba | Riešenie | macOS | Windows |
|---|---|---|---|
| Key-down/up + samotné modifikátory | `rdev` listener | Accessibility permission | funguje priamo |
| Bublina nekradne fokus | NSPanel (plugin `tauri-nspanel`) | ✓ | `WS_EX_NOACTIVATE` / focusable(false) |
| Paste na kurzor | `enigo` + clipboard swap | Accessibility permission | funguje priamo |
| Mikrofón | `cpal` | Microphone permission | funguje priamo |

## 4. Tok jedného diktovania

1. **Key-down** na hotkey → stav `Recording`, bublina sa zobrazí, cpal
   začne capture. Dvojité ťuknutie (down-up-down do ~300 ms) prepne do
   toggle režimu — nahráva až do ďalšieho ťuknutia. Držanie > ~300 ms
   = push-to-talk (nahráva kým držíš).
2. Každých ~100 ms event s amplitúdou → waveform v bubline.
3. Každé ~2,5 s sa kumulované audio (WAV) pošle na Groq → čiastočný
   prepis → živý text v bubline. (Whisper nie je streamovací; toto je
   štandardná simulácia.)
4. **Key-up / druhé ťuknutie** → stav `Transcribing` → celé audio sa pošle
   vcelku na Groq (finálny prepis je presnejší než zlepené chunky).
   Jazyk: auto-detekcia, alebo `language=sk|cs|en` pri manuálnom override.
5. Stav `Cleaning` → surový text na Meridian (`POST /v1/messages`,
   streaming off; model konfigurovateľný v nastaveniach, default preberá
   Meridian). Prompt: odstráň výplňové slová, doplň interpunkciu, oprav
   preklepy, zachovaj jazyk aj význam, nič nepridávaj.
   Timeout 5 s → fallback na surový text.
6. Stav `Injecting` → uloženie aktuálnej schránky → vloženie textu do
   schránky → simulácia Cmd/Ctrl+V → obnovenie pôvodnej schránky.
   Bublina ukáže ✓, fade-out, záznam do histórie. Audio buffer sa zahodí.

Zrušenie: Esc alebo klik na bublinu počas nahrávania → zahodiť, stav `Idle`.

## 5. UX

### Bublina (pilulka dole v strede, draggable, pozícia sa pamätá)

Stavy: skrytá (idle) → waveform + časovač (počúvam) → waveform + živý text
(prepisujem) → „✨ upravujem text…" (čistím) → „✓ vložené" + fade-out →
chybový stav (červený nádych + akcia). Nikdy nekradne fokus.

### Hlavné okno — sidebar s tromi sekciami

1. **História** — vyčistený text (rozklik = surový), čas, jazyk, dĺžka;
   kopírovať / zmazať / zmazať všetko; fulltextové hľadanie.
2. **Nastavenia** — hotkey (capture UI), režim jazyka (Auto/SK/EN/CS),
   čistenie (zap/vyp + štýl: jemné = len výplňové slová a interpunkcia,
   silné = aj mierne preformulovanie), Groq API kľúč (test pripojenia,
   uložený v OS keychain), Meridian URL + live status, autostart, téma.
3. **Setup wizard** (prvé spustenie) — Vitaj → Povolenia (mikrofón +
   accessibility, návod podľa OS) → Groq kľúč (link na console.groq.com)
   → Meridian (auto-detekcia, preskočiteľné) → skúšobné diktovanie.

### Dizajn

Moderný minimalizmus (Linear / Raycast / Vercel): neutrálna paleta s jedným
akcentom, dark + light mode, veľkorysé medzery, 1px bordery, mikro-animácie
(fade, spring). Bublina s priesvitným blur pozadím.

## 6. Error handling — zásada: nikdy nestratiť text

| Situácia | Správanie |
|---|---|
| Meridian nebeží / timeout | vloží sa surový text, bublina „vložené bez úprav" |
| Groq nedostupný / offline | chybová bublina, audio sa drží, „skúsiť znova" |
| Groq rate limit | to isté + hint |
| Paste zlyhá (chýba permission) | text do schránky + notifikácia „vlož Cmd+V" |
| Chýba mikrofónové povolenie | bublina s deep-linkom do systémových nastavení |
| Prázdny prepis | „nič som nepočul", nič sa nevkladá |

## 7. Dáta a súkromie

- Groq kľúč: OS keychain (crate `keyring`), nikdy plaintext.
- História: lokálna SQLite v app data adresári; mazanie jednotlivo aj hromadne.
- Nastavenia: JSON v app data adresári.
- Audio: len v pamäti, po prepise zahodené.
- Von idú dáta len na: Groq (audio) a Meridian→Anthropic (text na čistenie).

## 8. Testovanie

- Rust unit testy: stavový automat, double-tap detekcia, audio chunker.
- STT/cleanup klienti proti mock HTTP serveru.
- UI + permissions: manuálny checklist (globálne hotkeys a systémové dialógy
  sa automatizovať nedajú).
- Finálne overenie: reálne diktovanie end-to-end na macOS.

## 9. Repo a distribúcia

```
local-wispr-flow/
├─ src/                  # React + TS
│  ├─ windows/bubble/
│  ├─ windows/main/      # História, Nastavenia, Wizard
│  └─ shared/            # typy, dizajn tokeny
├─ src-tauri/src/        # audio, hotkey, stt, cleanup, inject, store, state
├─ .github/workflows/    # release: .dmg (universal) + .msi
└─ README.md
```

GitHub Releases pri tagu; bez code-signingu — README vysvetlí Gatekeeper
(`xattr -d com.apple.quarantine`) a SmartScreen. Dev: `pnpm tauri dev`.

## 10. Poradie implementácie

1. Jadro pipeline bez UI: hotkey → nahrávka → Groq → paste (macOS)
2. Bublina + waveform
3. Živý čiastočný prepis
4. Meridian cleanup + fallbacky
5. Hlavné okno: Nastavenia → História → Wizard
6. Windows podpora + doladenie
7. CI build + README + release

## 11. Mimo rozsahu (zámerne)

- Lokálny STT model (užívateľ výslovne nechce)
- Code signing / notarizácia (platené)
- Vlastné slovníky, per-app pravidlá, kontextové čistenie podľa appky
- Linux (dá sa doplniť neskôr, Tauri to umožňuje)
