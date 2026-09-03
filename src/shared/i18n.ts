/**
 * UI strings for the app windows. English is the default; the user can switch
 * to Slovak in Settings (`ui_language`). Backend-produced messages (bubble
 * status lines, history errors) are localised on the Rust side.
 */
import { useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type Settings, type UiLanguage } from "./ipc";
import { EVENT_SETTINGS_CHANGED } from "./events";

export type { UiLanguage };

const STRINGS = {
  // ---- sidebar / shell ----
  "nav.settings": { en: "Settings", sk: "Nastavenia" },
  "nav.history": { en: "History", sk: "História" },
  "warn.accessibility.label": { en: "Accessibility", sk: "Prístupnosť" },
  "warn.accessibility.detail": {
    en: "Dikto doesn't have the Accessibility permission, so dictated text can't be inserted.",
    sk: "Aplikácia nemá povolenie Asistenčný prístup — vkladanie nadiktovaného textu preto nebude fungovať.",
  },
  "warn.accessibility.action": { en: "Open System Settings", sk: "Otvoriť nastavenia systému" },
  "warn.pipelineDead.label": { en: "Dictation unavailable", sk: "Diktovanie nedostupné" },

  // ---- settings page ----
  "settings.title": { en: "Settings", sk: "Nastavenia" },
  "settings.subtitle": {
    en: "Hotkey, languages, text cleanup and the Groq key.",
    sk: "Klávesová skratka, jazyk, čistenie textu a Groq kľúč.",
  },
  "settings.loadError": {
    en: "Couldn't load settings. Try restarting the app.",
    sk: "Nepodarilo sa načítať nastavenia. Skús reštartovať appku.",
  },
  "settings.loading": { en: "Loading settings…", sk: "Načítavam nastavenia…" },
  "settings.saved": { en: "Saved", sk: "Uložené" },
  "settings.saveError": {
    en: "Couldn't save the change — please try again.",
    sk: "Nepodarilo sa uložiť zmenu — skús to znova.",
  },
  "settings.uiLanguage.title": { en: "Interface language", sk: "Jazyk rozhrania" },
  "settings.uiLanguage.desc": {
    en: "Language of the app's windows, bubble and tray. Dictation language is set below.",
    sk: "Jazyk okien, bubliny a ikony v lište. Jazyk diktovania sa nastavuje nižšie.",
  },
  "settings.uiLanguage.label": { en: "Interface language", sk: "Jazyk rozhrania" },
  "settings.hotkey.title": { en: "Hotkey", sk: "Klávesa" },
  "settings.hotkey.desc": {
    en: "Hold to record; double-tap to lock recording on.",
    sk: "Podrž pre nahrávanie, dvojité ťuknutie zamkne nahrávanie zapnuté.",
  },
  "settings.hotkey.label": { en: "Keyboard shortcut", sk: "Klávesová skratka" },
  "settings.hotkey.captureHint": { en: "press a key… (Esc to cancel)", sk: "stlač klávesu… (Esc = zrušiť)" },
  "settings.hotkey.cancel": { en: "Cancel", sk: "Zrušiť" },
  "settings.hotkey.change": { en: "Change", sk: "Zmeniť" },
  "settings.hotkey.stalled": {
    en: "Key not detected? Check the Accessibility permission",
    sk: "Klávesa nezachytená? Skontroluj povolenie Prístupnosť",
  },
  "settings.hotkey.openSettings": { en: "Open settings", sk: "Otvoriť nastavenia" },
  "settings.language.title": { en: "Dictation language", sk: "Jazyk diktovania" },
  "settings.language.desc": {
    en: "Language used for speech-to-text.",
    sk: "Jazyk diktovania pre prepis reči.",
  },
  "settings.language.label": { en: "Language", sk: "Jazyk" },
  "settings.language.auto": { en: "Auto", sk: "Auto" },
  "settings.cleanup.title": { en: "Text cleanup", sk: "Čistenie textu" },
  "settings.cleanup.desc": {
    en: "Claude fixes punctuation and flow before the text is inserted. Optional — without it the raw transcript is inserted.",
    sk: "Claude doladí interpunkciu a plynulosť prepisu pred vložením. Voliteľné — bez neho sa vloží surový prepis.",
  },
  "settings.cleanup.enabled": { en: "Cleanup enabled", sk: "Zapnuté čistenie" },
  "settings.cleanup.toggleAria": { en: "Text cleanup", sk: "Čistenie textu" },
  "settings.cleanup.style": { en: "Cleanup style", sk: "Štýl úprav" },
  "settings.cleanup.styleDesc": {
    en: "Strong lightly rephrases sentences for fluency.",
    sk: "Silné mierne preformuluje vety kvôli plynulosti.",
  },
  "settings.cleanup.styleLight": { en: "light", sk: "jemné" },
  "settings.cleanup.styleStrong": { en: "strong", sk: "silné" },
  "settings.cleanup.model": { en: "Claude model", sk: "Claude model" },
  "settings.cleanup.modelDesc": {
    en: "Cleans up the transcript — punctuation, filler words. Doesn't go through Groq.",
    sk: "Čistenie prepísaného textu — interpunkcia, výplňové slová. Nejde cez Groq.",
  },
  "settings.cleanup.modelFromMeridian": {
    en: "List loaded from Meridian.",
    sk: "Zoznam načítaný z Meridianu.",
  },
  "settings.cleanup.modelManual": {
    en: "Meridian is offline — type the model id; the list appears once it's running.",
    sk: "Meridian nebeží — zadaj id modelu; zoznam sa zobrazí, keď pobeží.",
  },
  "settings.cleanup.meridianUrl": { en: "Meridian URL", sk: "Meridian URL" },
  "settings.cleanup.testing": { en: "Testing…", sk: "Testujem…" },
  "settings.cleanup.test": { en: "Test", sk: "Otestovať" },
  "settings.cleanup.online": { en: "Meridian is running", sk: "Meridian beží" },
  "settings.cleanup.offline": { en: "Meridian unreachable", sk: "Meridian nedostupný" },
  "settings.cleanup.checking": { en: "Checking…", sk: "Zisťujem stav…" },
  "settings.cleanup.testOk": { en: "Works — the model answered.", sk: "Funguje — model odpovedal." },
  "settings.cleanup.testFail": { en: "Test failed.", sk: "Test zlyhal." },
  "settings.recordings.title": { en: "Recordings", sk: "Nahrávky" },
  "settings.recordings.desc": {
    en: "Every dictation is written to disk before it is transcribed, so nothing is lost even if Groq fails.",
    sk: "Každé diktovanie sa uloží na disk ešte pred prepisom, takže sa nestratí ani keď Groq zlyhá.",
  },
  "settings.retention.label": { en: "Keep history", sk: "Držať históriu" },
  "settings.retention.desc": {
    en: "Older completed dictations are deleted along with their recording. Failed and untranscribed recordings are never deleted automatically — only manually from History.",
    sk: "Staršie dokončené diktáty sa zmažú aj s nahrávkou. Zlyhané a neprepísané nahrávky sa nemažú nikdy — tie zmažeš len ručne v histórii.",
  },
  "settings.retention.days7": { en: "7 days", sk: "7 dní" },
  "settings.retention.days30": { en: "30 days", sk: "30 dní" },
  "settings.retention.forever": { en: "forever", sk: "navždy" },
  "settings.groq.title": { en: "Groq API key", sk: "Groq API kľúč" },
  "settings.groq.desc": {
    en: "Speech-to-text (Whisper via Groq). Free at console.groq.com.",
    sk: "Prepis reči na text (Whisper cez Groq). Zadarmo na console.groq.com.",
  },
  "settings.groq.label": { en: "API key", sk: "API kľúč" },
  "settings.groq.stored": { en: "Key is stored in local settings", sk: "Kľúč je uložený v lokálnom nastavení" },
  "settings.groq.placeholderStored": {
    en: "•••••••• (saved — paste a new one to replace)",
    sk: "•••••••• (uložený — vlož nový pre zmenu)",
  },
  "settings.groq.save": { en: "Save", sk: "Uložiť" },
  "settings.groq.test": { en: "Test", sk: "Otestovať" },
  "settings.groq.saved": { en: "key saved", sk: "kľúč bol uložený" },
  "settings.groq.testing": { en: "testing connection…", sk: "testujem spojenie…" },
  "settings.groq.testOk": { en: "✓ connection works", sk: "✓ spojenie funguje" },
  "settings.groq.testFail": { en: "✗ connection failed", sk: "✗ spojenie zlyhalo" },
  "settings.system.title": { en: "System", sk: "Systém" },
  "settings.system.autostart": { en: "Launch at login", sk: "Spustiť pri prihlásení" },
  "settings.system.autostartError": { en: "couldn't change — try again", sk: "nepodarilo sa zmeniť — skús znova" },
  "key.rightOption": { en: "Right Option (⌥)", sk: "Pravý Option (⌥)" },
  "key.rightAlt": { en: "Right Alt (AltGr)", sk: "Pravý Alt (AltGr)" },
  "key.leftAlt": { en: "Left Alt", sk: "Ľavý Alt" },
  "key.leftOption": { en: "Left Option (⌥)", sk: "Ľavý Option (⌥)" },
  "key.rightCtrl": { en: "Right Ctrl", sk: "Pravý Ctrl" },
  "key.leftCtrl": { en: "Left Ctrl", sk: "Ľavý Ctrl" },
  "key.rightCmd": { en: "Right Cmd (⌘)", sk: "Pravý Cmd (⌘)" },
  "key.leftCmd": { en: "Left Cmd (⌘)", sk: "Ľavý Cmd (⌘)" },
  "key.rightWin": { en: "Right Win", sk: "Pravý Win" },
  "key.leftWin": { en: "Left Win", sk: "Ľavý Win" },
  "key.rightShift": { en: "Right Shift", sk: "Pravý Shift" },
  "key.leftShift": { en: "Left Shift", sk: "Ľavý Shift" },
  "key.space": { en: "Space", sk: "Medzerník" },

  // ---- history page ----
  "history.title": { en: "History", sk: "História" },
  "history.subtitle": { en: "Everything you've dictated so far.", sk: "Zoznam tvojich doterajších diktovaní." },
  "history.searchPlaceholder": { en: "Search history…", sk: "Hľadať v histórii…" },
  "history.clearSearch": { en: "Clear search", sk: "Vymazať hľadanie" },
  "history.count.one": { en: "{n} dictation", sk: "{n} diktovanie" },
  "history.count.few": { en: "{n} dictations", sk: "{n} diktovania" },
  "history.count.many": { en: "{n} dictations", sk: "{n} diktovaní" },
  "history.clearAll": { en: "Delete all", sk: "Zmazať všetko" },
  "history.clearAllConfirm": { en: "Really delete everything?", sk: "Naozaj zmazať všetko?" },
  "history.loadError": {
    en: "Couldn't load history. Try restarting the app.",
    sk: "Nepodarilo sa načítať históriu. Skús reštartovať appku.",
  },
  "history.loading": { en: "Loading history…", sk: "Načítavam históriu…" },
  "history.noResults.title": { en: "Nothing found", sk: "Nič sa nenašlo" },
  "history.noResults.hint": { en: "No dictation matches “{q}”.", sk: "Pre „{q}“ sme nenašli žiadne diktovanie." },
  "history.empty.title": { en: "No dictations yet…", sk: "Zatiaľ žiadne diktovania…" },
  "history.empty.hint": {
    en: "Hold the hotkey and start talking — your transcripts will show up here.",
    sk: "Podrž klávesovú skratku a začni diktovať — tvoje prepisy sa objavia tu.",
  },
  "history.row.transcribing": { en: "Transcribing…", sk: "Prepisujem…" },
  "history.row.noTranscript": { en: "No transcript — recording is saved", sk: "Bez prepisu — nahrávka je uložená" },
  "history.row.failed": { en: "Failed", sk: "Zlyhalo" },
  "history.row.pending": { en: "Transcribing", sk: "Prepisujem" },
  "history.row.audioSaved": { en: "Recording is saved", sk: "Nahrávka je uložená" },
  "history.row.raw": { en: "Raw transcript", sk: "Surový prepis" },
  "history.row.retry": { en: "Transcribe again", sk: "Prepísať znova" },
  "history.row.export": { en: "Download audio", sk: "Stiahnuť audio" },
  "history.row.copied": { en: "✓ Copied", sk: "✓ Skopírované" },
  "history.row.copy": { en: "Copy", sk: "Kopírovať" },
  "history.row.collapse": { en: "Collapse", sk: "Zbaliť" },
  "history.row.expand": { en: "Expand", sk: "Rozbaliť" },
  "history.row.delete": { en: "Delete", sk: "Zmazať" },
  "history.retryFailed": { en: "transcription failed again", sk: "prepis znova zlyhal" },
  "history.exported": { en: "✓ saved: {file}", sk: "✓ uložené: {file}" },
  "history.exportFailed": { en: "saving failed", sk: "uloženie zlyhalo" },
  "time.justNow": { en: "just now", sk: "práve teraz" },
  "time.minutesAgo": { en: "{n} min ago", sk: "pred {n} min" },
  "time.hoursAgo": { en: "{n} h ago", sk: "pred {n} h" },
  "time.yesterday": { en: "yesterday", sk: "včera" },

  // ---- wizard ----
  "wizard.skipAll": { en: "Skip setup", sk: "Preskočiť sprievodcu" },
  "wizard.progress": { en: "Setup progress", sk: "Priebeh sprievodcu" },
  "wizard.next": { en: "Next", sk: "Ďalej" },
  "wizard.skip": { en: "Skip", sk: "Preskočiť" },
  "wizard.finish": { en: "Finish", sk: "Dokončiť" },
  "wizard.back": { en: "Back", sk: "Späť" },
  "wizard.welcome.eyebrow": { en: "Welcome", sk: "Vitaj" },
  "wizard.welcome.desc": {
    en: "Hold the hotkey, say what you need, and the text appears right where you're typing — in mail, in your editor, wherever the cursor is.",
    sk: "Podrž klávesovú skratku, povedz čo potrebuješ, a text sa objaví presne tam, kde práve píšeš — v mailoch, v editore, kdekoľvek má kurzor fokus.",
  },
  "wizard.welcome.keycapLabel": { en: "right", sk: "pravý" },
  "wizard.permissions.eyebrow": { en: "Permissions", sk: "Povolenia" },
  "wizard.permissions.title": { en: "Check access permissions", sk: "Over prístupové oprávnenia" },
  "wizard.permissions.descMac": {
    en: "Dikto needs system permissions to insert dictated text and to record from the microphone.",
    sk: "Appka potrebuje systémové povolenia, aby vedela vkladať nadiktovaný text a nahrávať mikrofón.",
  },
  "wizard.permissions.descWin": {
    en: "Windows will ask for microphone access the first time you record — just allow it.",
    sk: "Windows sa pri prvom nahrávaní opýta na prístup k mikrofónu — stačí ho povoliť.",
  },
  "wizard.permissions.accessibility": { en: "Accessibility", sk: "Asistenčný prístup" },
  "wizard.permissions.granted": { en: "granted", sk: "povolené" },
  "wizard.permissions.neededForPaste": { en: "needed to insert text", sk: "potrebné pre vkladanie textu" },
  "wizard.permissions.open": { en: "Open settings", sk: "Otvoriť nastavenia" },
  "wizard.permissions.microphone": { en: "Microphone", sk: "Mikrofón" },
  "wizard.permissions.micHint": { en: "requested on first dictation", sk: "zistí sa pri prvom diktovaní" },
  "wizard.permissions.devNote": {
    en: "In dev mode (pnpm tauri dev) these permissions belong to the terminal, not to this app — check them for Terminal/iTerm in System Settings.",
    sk: "V dev režime (pnpm tauri dev) drží tieto povolenia terminál, nie táto appka — skontroluj ich pre Terminal/iTerm v Nastaveniach systému.",
  },
  "wizard.groq.eyebrow": { en: "Groq key", sk: "Groq kľúč" },
  "wizard.groq.title": { en: "Set up speech-to-text", sk: "Priprav prepis reči" },
  "wizard.groq.desc": {
    en: "Transcription runs on Groq Whisper — the free tier is enough for everyday dictation. Create an account and paste the generated API key below.",
    sk: "Prepis hlasu beží cez Groq Whisper — bezplatný tier stačí na bežné diktovanie. Vytvor si účet a vlož si vygenerovaný API kľúč nižšie.",
  },
  "wizard.groq.open": { en: "Open console.groq.com ↗", sk: "Otvoriť console.groq.com ↗" },
  "wizard.groq.test": { en: "Test", sk: "Otestovať" },
  "wizard.groq.save": { en: "Save", sk: "Uložiť" },
  "wizard.groq.saved": { en: "✓ key saved", sk: "✓ kľúč uložený" },
  "wizard.groq.testing": { en: "testing connection…", sk: "testujem spojenie…" },
  "wizard.groq.testOk": { en: "✓ connection works", sk: "✓ spojenie funguje" },
  "wizard.groq.testFail": { en: "✗ connection failed", sk: "✗ spojenie zlyhalo" },
  "wizard.cleanup.eyebrow": { en: "Text cleanup (optional)", sk: "Čistenie textu (voliteľné)" },
  "wizard.cleanup.title": { en: "Polish the text", sk: "Doladenie textu" },
  "wizard.cleanup.desc": {
    en: "Meridian uses Claude to fix punctuation and flow before the text is inserted. It's optional — without it the raw Whisper transcript is inserted.",
    sk: "Meridian pred vložením opraví interpunkciu a plynulosť prepisu pomocou Claude. Je to voliteľné — bez neho sa vloží surový prepis z Whisperu.",
  },
  "wizard.cleanup.online": { en: "running and ready", sk: "beží a je pripravený" },
  "wizard.cleanup.offline": { en: "not reachable", sk: "nie je dostupný" },
  "wizard.cleanup.checking": { en: "checking…", sk: "zisťujem stav…" },
  "wizard.cleanup.retry": { en: "Try again", sk: "Skúsiť znova" },
  "wizard.cleanup.note": {
    en: "Start Meridian in a terminal with the command `meridian` and click “Try again”. Or just continue — dictation works without it.",
    sk: "Spusti Meridian v termináli príkazom `meridian` a klikni na „Skúsiť znova“. Alebo jednoducho pokračuj ďalej — diktovanie bude fungovať aj bez neho.",
  },
  "wizard.trial.eyebrow": { en: "Test dictation", sk: "Skúšobné diktovanie" },
  "wizard.trial.title": { en: "Try it live", sk: "Vyskúšaj to naživo" },
  "wizard.trial.desc": {
    en: "Click into the field below, hold the hotkey and say a few words — the text will appear right here.",
    sk: "Klikni do poľa nižšie, podrž klávesovú skratku a povedz pár slov — text sa objaví priamo tu.",
  },
  "wizard.trial.placeholder": { en: "click here, hold the key and talk…", sk: "klikni sem, podrž klávesu a hovor…" },
  "wizard.trial.success": { en: "✓ Great, it works!", sk: "✓ Super, funguje to!" },

  // ---- bubble ----
  "bubble.idleAria": { en: "Dikto is running", sk: "Dikto je aktívne" },
  "bubble.cancelTitle": { en: "Cancel (Esc)", sk: "Zrušiť (Esc)" },
  "bubble.transcribing": { en: "transcribing…", sk: "prepisujem…" },
  "bubble.cleaning": { en: "✨ cleaning up…", sk: "✨ upravujem text…" },
  "bubble.injecting": { en: "inserting…", sk: "vkladám…" },
  "bubble.retry": { en: "try again", sk: "skúsiť znova" },
  "bubble.savedHint": { en: "the recording is saved in History", sk: "nahrávka je uložená v histórii" },
} as const satisfies Record<string, { en: string; sk: string }>;

export type StringKey = keyof typeof STRINGS;

// ---- language store ----
let current: UiLanguage = "en";
const listeners = new Set<() => void>();

export function getLang(): UiLanguage {
  return current;
}

export function setLang(lang: UiLanguage): void {
  if (lang === current) return;
  current = lang;
  document.documentElement.lang = lang;
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Re-renders the caller whenever the UI language changes. */
export function useLang(): UiLanguage {
  return useSyncExternalStore(subscribe, getLang, getLang);
}

export function t(key: StringKey, vars?: Record<string, string | number>): string {
  let text: string = STRINGS[key][current];
  if (vars) {
    for (const [name, value] of Object.entries(vars)) {
      text = text.split(`{${name}}`).join(String(value));
    }
  }
  return text;
}

/** `t` bound to the current language, so a component both re-renders and reads fresh strings. */
export function useT(): typeof t {
  useLang();
  return t;
}

/**
 * Seeds the language from settings and follows `settings:changed`. Call once
 * per window before rendering; safe if settings can't be read (stays "en").
 */
export function initLang(): void {
  api
    .getSettings()
    .then((s) => setLang(s.ui_language))
    .catch(() => {});
  void listen<Settings>(EVENT_SETTINGS_CHANGED, (event) => setLang(event.payload.ui_language));
}
