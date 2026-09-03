import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  disable as autostartDisable,
  enable as autostartEnable,
  isEnabled as autostartIsEnabled,
} from "@tauri-apps/plugin-autostart";
import { api } from "../../../shared/ipc";
import type { CleanupStyle, LanguageMode, Settings, UiLanguage } from "../../../shared/ipc";
import { t, useT, type StringKey } from "../../../shared/i18n";
import { isMac } from "../../../shared/platform";
import { EVENT_HOTKEY_CAPTURED, EVENT_SETTINGS_CHANGED, type HotkeyCapturedPayload } from "../../../shared/events";
import "./settings.css";

const CAPTURE_TIMEOUT_MS = 10_000;
const CAPTURE_STALL_HINT_MS = 3_000;
const DEBOUNCE_MS = 500;
const MERIDIAN_POLL_MS = 10_000;
const GROQ_SAVED_FLASH_MS = 2_500;

type MeridianStatus = "unknown" | "online" | "offline";

// KeyboardEvent.code -> rdev's Key Debug string (verified against rdev 0.5.3's
// `Key` enum, ~/.cargo/registry/src/.../rdev-0.5.3/src/rdev.rs). Fallback path
// for when the Rust rdev listener dies silently (stale Accessibility TCC
// grant) — see hotkey.rs `capture_next` / `hotkey:captured`.
const CODE_TO_RDEV: Record<string, string> = {
  AltLeft: "Alt",
  AltRight: "AltGr", // right Option on mac; rdev has no separate AltRight variant
  ControlLeft: "ControlLeft",
  ControlRight: "ControlRight",
  MetaLeft: "MetaLeft",
  MetaRight: "MetaRight",
  ShiftLeft: "ShiftLeft",
  ShiftRight: "ShiftRight",
  Space: "Space",
};
for (let i = 1; i <= 12; i++) CODE_TO_RDEV[`F${i}`] = `F${i}`;
for (let c = 65; c <= 90; c++) {
  const letter = String.fromCharCode(c);
  CODE_TO_RDEV[`Key${letter}`] = `Key${letter}`;
}
for (let d = 0; d <= 9; d++) CODE_TO_RDEV[`Digit${d}`] = `Num${d}`;

const UI_LANGUAGE_OPTIONS: Array<{ id: UiLanguage; label: string }> = [
  { id: "en", label: "EN" },
  { id: "sk", label: "SK" },
];

const LANGUAGE_OPTIONS: Array<{ id: LanguageMode; label: string | StringKey }> = [
  { id: "auto", label: "settings.language.auto" },
  { id: "sk", label: "SK" },
  { id: "cs", label: "CS" },
  { id: "en", label: "EN" },
];

const CLEANUP_STYLE_OPTIONS: Array<{ id: CleanupStyle; label: StringKey }> = [
  { id: "light", label: "settings.cleanup.styleLight" },
  { id: "strong", label: "settings.cleanup.styleStrong" },
];

const RETENTION_OPTIONS: Array<{ id: number; label: StringKey }> = [
  { id: 7, label: "settings.retention.days7" },
  { id: 30, label: "settings.retention.days30" },
  { id: 0, label: "settings.retention.forever" },
];

/** Maps rdev key names to a readable, platform-aware label; other keys keep their name. */
const KEY_LABELS: Record<string, StringKey> = isMac
  ? {
      AltGr: "key.rightOption",
      Alt: "key.leftOption",
      ControlRight: "key.rightCtrl",
      ControlLeft: "key.leftCtrl",
      MetaRight: "key.rightCmd",
      MetaLeft: "key.leftCmd",
      ShiftRight: "key.rightShift",
      ShiftLeft: "key.leftShift",
      Space: "key.space",
    }
  : {
      AltGr: "key.rightAlt",
      Alt: "key.leftAlt",
      ControlRight: "key.rightCtrl",
      ControlLeft: "key.leftCtrl",
      MetaRight: "key.rightWin",
      MetaLeft: "key.leftWin",
      ShiftRight: "key.rightShift",
      ShiftLeft: "key.leftShift",
      Space: "key.space",
    };

export default function SettingsPage() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [saveError, setSaveError] = useState(false);
  useT();

  const settingsRef = useRef<Settings | null>(null);
  settingsRef.current = settings;

  // ---- initial load ----
  useEffect(() => {
    let cancelled = false;
    api
      .getSettings()
      .then((s) => {
        if (cancelled) return;
        setSettings(s);
        setLoaded(true);
      })
      .catch(() => {
        if (!cancelled) setLoadError(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // ---- cross-window sync (tray, other windows) ----
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen<Settings>(EVENT_SETTINGS_CHANGED, (event) => setSettings(event.payload)).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Optimistic patch: applies immediately, saves, rolls back + flags an
  // inline error banner if the backend rejects it.
  const commit = useCallback((partial: Partial<Settings>) => {
    const prev = settingsRef.current;
    if (!prev) return;
    const next = { ...prev, ...partial };
    setSettings(next);
    setSaveError(false);
    api.setSettings(next).catch(() => {
      setSettings(prev);
      setSaveError(true);
    });
  }, []);

  // ---- hotkey capture ----
  const [capturing, setCapturing] = useState(false);
  const [captureStalled, setCaptureStalled] = useState(false);
  const captureTimeoutRef = useRef<number | undefined>(undefined);
  const captureStallRef = useRef<number | undefined>(undefined);
  // Whichever source (Rust `hotkey:captured` event or the DOM keydown
  // fallback below) reports a key first wins the race; this ref guards
  // against both firing for the same physical keypress.
  const captureWonRef = useRef(false);

  const exitCapture = useCallback((disarmBackend: boolean) => {
    window.clearTimeout(captureTimeoutRef.current);
    window.clearTimeout(captureStallRef.current);
    setCapturing(false);
    setCaptureStalled(false);
    if (disarmBackend) void api.hotkeyCaptureStart(true);
  }, []);

  const finishCapture = useCallback(
    (key: string) => {
      window.clearTimeout(captureTimeoutRef.current);
      window.clearTimeout(captureStallRef.current);
      setCapturing(false);
      setCaptureStalled(false);
      commit({ hotkey: key });
    },
    [commit],
  );

  const startCapture = () => {
    captureWonRef.current = false;
    setCaptureStalled(false);
    setCapturing(true);
    void api.hotkeyCaptureStart(false);
    captureTimeoutRef.current = window.setTimeout(() => exitCapture(true), CAPTURE_TIMEOUT_MS);
    captureStallRef.current = window.setTimeout(() => setCaptureStalled(true), CAPTURE_STALL_HINT_MS);
  };

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen<HotkeyCapturedPayload>(EVENT_HOTKEY_CAPTURED, (event) => {
      if (captureWonRef.current) return;
      captureWonRef.current = true;
      finishCapture(event.payload.key);
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [finishCapture]);

  // DOM fallback: the global rdev listener thread can die silently (e.g. a
  // stale Accessibility TCC grant), leaving the Rust `hotkey:captured` event
  // never firing. While capturing, also read raw browser keydowns so the UI
  // still works even when the OS-level listener is dead. Escape cancels.
  useEffect(() => {
    if (!capturing) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        exitCapture(true);
        return;
      }
      const rdevName = CODE_TO_RDEV[e.code];
      if (!rdevName || captureWonRef.current) return;
      e.preventDefault();
      captureWonRef.current = true;
      finishCapture(rdevName);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [capturing, exitCapture, finishCapture]);

  // Whenever capture mode ends — success, Escape, timeout, or the component
  // unmounting mid-capture (e.g. navigating to another page) — make sure the
  // Rust one-shot `capture_next` flag doesn't linger armed.
  useEffect(() => {
    if (!capturing) return;
    return () => void api.hotkeyCaptureStart(true);
  }, [capturing]);

  useEffect(() => () => {
    window.clearTimeout(captureTimeoutRef.current);
    window.clearTimeout(captureStallRef.current);
  }, []);

  // ---- languages ----
  const setLanguage = (language: LanguageMode) => commit({ language });
  const setUiLanguage = (ui_language: UiLanguage) => commit({ ui_language });

  // ---- cleanup: toggle, style, model + meridian url (debounced) ----
  const toggleCleanup = () => {
    const s = settingsRef.current;
    if (s) commit({ cleanup_enabled: !s.cleanup_enabled });
  };
  const setCleanupStyle = (cleanup_style: CleanupStyle) => commit({ cleanup_style });
  const setRetention = (history_retention_days: number) => commit({ history_retention_days });

  const [modelDraft, setModelDraft] = useState("");
  const [meridianDraft, setMeridianDraft] = useState("");
  const modelFocused = useRef(false);
  const meridianFocused = useRef(false);
  const modelTimer = useRef<number | undefined>(undefined);
  const meridianTimer = useRef<number | undefined>(undefined);

  // Result of the last "Test" click, or null when nothing has been tried
  // since the Meridian URL changed.
  const [cleanupTest, setCleanupTest] = useState<{ ok: boolean; text: string } | null>(null);
  const [testingCleanup, setTestingCleanup] = useState(false);

  useEffect(() => {
    if (!settings) return;
    if (!modelFocused.current) setModelDraft(settings.cleanup_model);
    if (!meridianFocused.current) setMeridianDraft(settings.meridian_url);
  }, [settings]);

  const runCleanupTest = () => {
    // Meridian URL edits are debounced; land them before the backend reads it.
    flushMeridian();
    setTestingCleanup(true);
    setCleanupTest(null);
    window.setTimeout(() => {
      api
        .testCleanup()
        .then(() => setCleanupTest({ ok: true, text: t("settings.cleanup.testOk") }))
        .catch((e) => setCleanupTest({ ok: false, text: typeof e === "string" ? e : t("settings.cleanup.testFail") }))
        .finally(() => setTestingCleanup(false));
    }, 50);
  };

  const [meridianStatus, setMeridianStatus] = useState<MeridianStatus>("unknown");
  const refreshMeridian = useCallback(() => {
    api
      .meridianStatus()
      .then((ok) => setMeridianStatus(ok ? "online" : "offline"))
      .catch(() => setMeridianStatus("offline"));
  }, []);

  useEffect(() => {
    if (!loaded) return;
    refreshMeridian();
    const id = window.setInterval(refreshMeridian, MERIDIAN_POLL_MS);
    return () => window.clearInterval(id);
  }, [loaded, refreshMeridian]);

  const commitModelDebounced = (value: string) => {
    setModelDraft(value);
    window.clearTimeout(modelTimer.current);
    modelTimer.current = window.setTimeout(() => commit({ cleanup_model: value }), DEBOUNCE_MS);
  };
  const flushModel = () => {
    window.clearTimeout(modelTimer.current);
    const prev = settingsRef.current;
    if (prev && modelDraft !== prev.cleanup_model) commit({ cleanup_model: modelDraft });
  };

  const commitMeridianDebounced = (value: string) => {
    setMeridianDraft(value);
    window.clearTimeout(meridianTimer.current);
    meridianTimer.current = window.setTimeout(() => {
      commit({ meridian_url: value });
      setCleanupTest(null);
      refreshMeridian();
    }, DEBOUNCE_MS);
  };
  const flushMeridian = () => {
    window.clearTimeout(meridianTimer.current);
    const prev = settingsRef.current;
    if (prev && meridianDraft !== prev.meridian_url) {
      commit({ meridian_url: meridianDraft });
      refreshMeridian();
    }
  };

  useEffect(
    () => () => {
      window.clearTimeout(modelTimer.current);
      window.clearTimeout(meridianTimer.current);
    },
    [],
  );

  // ---- groq api key ----
  const [hasGroqKey, setHasGroqKey] = useState(false);
  const [groqDraft, setGroqDraft] = useState("");
  const [groqSaving, setGroqSaving] = useState(false);
  const [groqSaved, setGroqSaved] = useState(false);
  const [groqTest, setGroqTest] = useState<"idle" | "testing" | "ok" | "fail">("idle");
  const groqSavedTimer = useRef<number | undefined>(undefined);

  const refreshGroqKeyStatus = useCallback(() => {
    api
      .hasGroqKey()
      .then(setHasGroqKey)
      .catch(() => {});
  }, []);

  useEffect(refreshGroqKeyStatus, [refreshGroqKeyStatus]);
  useEffect(() => () => window.clearTimeout(groqSavedTimer.current), []);

  const saveGroqKey = () => {
    const key = groqDraft.trim();
    if (!key) return;
    setGroqSaving(true);
    api
      .setGroqKey(key)
      .then(() => {
        setGroqDraft("");
        setGroqTest("idle");
        setGroqSaved(true);
        refreshGroqKeyStatus();
        window.clearTimeout(groqSavedTimer.current);
        groqSavedTimer.current = window.setTimeout(() => setGroqSaved(false), GROQ_SAVED_FLASH_MS);
      })
      .catch(() => setSaveError(true))
      .finally(() => setGroqSaving(false));
  };

  const testGroqKey = () => {
    setGroqTest("testing");
    api
      .testGroqKey()
      .then((ok) => setGroqTest(ok ? "ok" : "fail"))
      .catch(() => setGroqTest("fail"));
  };

  // ---- autostart ----
  const [autostartOn, setAutostartOn] = useState(false);
  const [autostartBusy, setAutostartBusy] = useState(false);
  const [autostartError, setAutostartError] = useState(false);

  useEffect(() => {
    autostartIsEnabled()
      .then(setAutostartOn)
      .catch(() => {});
  }, []);

  const toggleAutostart = () => {
    const next = !autostartOn;
    setAutostartOn(next);
    setAutostartBusy(true);
    setAutostartError(false);
    (next ? autostartEnable() : autostartDisable())
      .then(() => commit({ autostart: next }))
      .catch(() => {
        setAutostartOn(!next);
        setAutostartError(true);
      })
      .finally(() => setAutostartBusy(false));
  };

  if (loadError) {
    return (
      <div className="settings">
        <div className="settings__banner">{t("settings.loadError")}</div>
      </div>
    );
  }
  if (!settings) {
    return (
      <div className="settings">
        <p className="settings__loading">{t("settings.loading")}</p>
      </div>
    );
  }

  return (
    <div className="settings">
      <header className="settings__header">
        <h1 className="settings__title">{t("settings.title")}</h1>
        <p className="settings__subtitle">{t("settings.subtitle")}</p>
      </header>

      {saveError && <div className="settings__banner">{t("settings.saveError")}</div>}

      <section className="settings-section">
        <div className="settings-section__head">
          <h2 className="settings-section__title">{t("settings.uiLanguage.title")}</h2>
          <p className="settings-section__desc">{t("settings.uiLanguage.desc")}</p>
        </div>
        <div className="settings-row">
          <span className="settings-row__label">{t("settings.uiLanguage.label")}</span>
          <div className="settings-row__control">
            <div className="segmented" role="tablist" aria-label={t("settings.uiLanguage.label")}>
              {UI_LANGUAGE_OPTIONS.map((opt) => (
                <button
                  key={opt.id}
                  type="button"
                  role="tab"
                  aria-selected={settings.ui_language === opt.id}
                  className={`segmented__option${settings.ui_language === opt.id ? " is-active" : ""}`}
                  onClick={() => setUiLanguage(opt.id)}
                >
                  {opt.label}
                </button>
              ))}
            </div>
          </div>
        </div>
      </section>

      <section className="settings-section">
        <div className="settings-section__head">
          <h2 className="settings-section__title">{t("settings.hotkey.title")}</h2>
          <p className="settings-section__desc">{t("settings.hotkey.desc")}</p>
        </div>
        <div className="settings-row">
          <div className="settings-row__text">
            <span className="settings-row__label">{t("settings.hotkey.label")}</span>
            {capturing && <span className="capture-hint">{t("settings.hotkey.captureHint")}</span>}
          </div>
          <div className="settings-row__control">
            <span className={`keycap${capturing ? " keycap--capturing" : ""}`}>
              {capturing ? "…" : humanizeKey(settings.hotkey)}
            </span>
            {capturing ? (
              <button type="button" className="btn" onClick={() => exitCapture(true)}>
                {t("settings.hotkey.cancel")}
              </button>
            ) : (
              <button type="button" className="btn" onClick={startCapture}>
                {t("settings.hotkey.change")}
              </button>
            )}
          </div>
        </div>
        {capturing && captureStalled && (
          <div className="settings-row settings-row--tight">
            <span className="inline-note">{t("settings.hotkey.stalled")}</span>
            <button
              type="button"
              className="btn"
              onClick={() => void api.openPrivacySettings("accessibility")}
            >
              {t("settings.hotkey.openSettings")}
            </button>
          </div>
        )}
      </section>

      <section className="settings-section">
        <div className="settings-section__head">
          <h2 className="settings-section__title">{t("settings.language.title")}</h2>
          <p className="settings-section__desc">{t("settings.language.desc")}</p>
        </div>
        <div className="settings-row">
          <span className="settings-row__label">{t("settings.language.label")}</span>
          <div className="settings-row__control">
            <div className="segmented" role="tablist" aria-label={t("settings.language.label")}>
              {LANGUAGE_OPTIONS.map((opt) => (
                <button
                  key={opt.id}
                  type="button"
                  role="tab"
                  aria-selected={settings.language === opt.id}
                  className={`segmented__option${settings.language === opt.id ? " is-active" : ""}`}
                  onClick={() => setLanguage(opt.id)}
                >
                  {opt.id === "auto" ? t("settings.language.auto") : opt.label}
                </button>
              ))}
            </div>
          </div>
        </div>
      </section>

      <section className="settings-section">
        <div className="settings-section__head">
          <h2 className="settings-section__title">{t("settings.cleanup.title")}</h2>
          <p className="settings-section__desc">{t("settings.cleanup.desc")}</p>
        </div>
        <div className="settings-row">
          <span className="settings-row__label">{t("settings.cleanup.enabled")}</span>
          <div className="settings-row__control">
            <Toggle checked={settings.cleanup_enabled} onChange={toggleCleanup} label={t("settings.cleanup.toggleAria")} />
          </div>
        </div>


        <div className="settings-row">
          <div className="settings-row__text">
            <span className="settings-row__label">{t("settings.cleanup.style")}</span>
            <span className="settings-row__desc">{t("settings.cleanup.styleDesc")}</span>
          </div>
          <div className="settings-row__control">
            <RadioGroup
              value={settings.cleanup_style}
              disabled={!settings.cleanup_enabled}
              options={CLEANUP_STYLE_OPTIONS}
              onChange={setCleanupStyle}
              name="cleanup-style"
            />
          </div>
        </div>

        <div className="settings-row settings-row--column">
          <span className="settings-row__label">{t("settings.cleanup.model")}</span>
          <span className="settings-row__desc">{t("settings.cleanup.modelDesc")}</span>
          <div className="settings-row__control">
            <input
              className="field"
              value={modelDraft}
              onFocus={() => (modelFocused.current = true)}
              onBlur={() => {
                modelFocused.current = false;
                flushModel();
              }}
              onChange={(e) => commitModelDebounced(e.target.value)}
              placeholder="claude-sonnet-5"
              spellCheck={false}
            />
          </div>
        </div>

        <div className="settings-row settings-row--column">
          <span className="settings-row__label">{t("settings.cleanup.meridianUrl")}</span>
          <div className="settings-row__control">
            <div className="field-row">
              <input
                className="field"
                value={meridianDraft}
                onFocus={() => (meridianFocused.current = true)}
                onBlur={() => {
                  meridianFocused.current = false;
                  flushMeridian();
                }}
                onChange={(e) => commitMeridianDebounced(e.target.value)}
                placeholder="http://127.0.0.1:3456"
                spellCheck={false}
              />
              <button type="button" className="btn" disabled={testingCleanup} onClick={runCleanupTest}>
                {testingCleanup ? t("settings.cleanup.testing") : t("settings.cleanup.test")}
              </button>
            </div>
          </div>
        </div>
        <div className="settings-row settings-row--tight">
          <span className="status-line">
            <StatusDot status={meridianStatus} />
            {meridianStatus === "online" && t("settings.cleanup.online")}
            {meridianStatus === "offline" && t("settings.cleanup.offline")}
            {meridianStatus === "unknown" && t("settings.cleanup.checking")}
          </span>
        </div>
        {/* The dot only proves something is listening; this proves it answers. */}
        {cleanupTest && (
          <div className="settings-row settings-row--tight">
            <span className="status-line">
              <span
                className={`status-dot status-dot--${cleanupTest.ok ? "ok" : "fail"}`}
                aria-hidden
              />
              {cleanupTest.text}
            </span>
          </div>
        )}
      </section>

      <section className="settings-section">
        <div className="settings-section__head">
          <h2 className="settings-section__title">{t("settings.recordings.title")}</h2>
          <p className="settings-section__desc">{t("settings.recordings.desc")}</p>
        </div>
        <div className="settings-row">
          <div className="settings-row__text">
            <span className="settings-row__label">{t("settings.retention.label")}</span>
            <span className="settings-row__desc">{t("settings.retention.desc")}</span>
          </div>
          <div className="settings-row__control">
            <RadioGroup
              value={settings.history_retention_days}
              options={RETENTION_OPTIONS}
              onChange={setRetention}
              name="history-retention"
            />
          </div>
        </div>
      </section>

      <section className="settings-section">
        <div className="settings-section__head">
          <h2 className="settings-section__title">{t("settings.groq.title")}</h2>
          <p className="settings-section__desc">{t("settings.groq.desc")}</p>
        </div>
        <div className="settings-row settings-row--column">
          <span className="settings-row__label">{t("settings.groq.label")}</span>
          {hasGroqKey && (
            <span className="status-line">
              <span className="status-dot status-dot--ok" aria-hidden />
              {t("settings.groq.stored")}
            </span>
          )}
          <div className="settings-row__control">
            <div className="field-row">
              <input
                type="password"
                className="field"
                value={groqDraft}
                onChange={(e) => setGroqDraft(e.target.value)}
                placeholder={hasGroqKey ? t("settings.groq.placeholderStored") : "gsk_…"}
                autoComplete="off"
                spellCheck={false}
              />
              <button
                type="button"
                className="btn btn--primary"
                onClick={saveGroqKey}
                disabled={groqSaving || !groqDraft.trim()}
              >
                {t("settings.groq.save")}
              </button>
              <button
                type="button"
                className="btn"
                onClick={testGroqKey}
                disabled={!hasGroqKey || groqTest === "testing"}
              >
                {t("settings.groq.test")}
              </button>
            </div>
          </div>
        </div>
        <div className="settings-row settings-row--tight">
          <span className={`inline-note${groqTest === "fail" ? "" : " inline-note--muted"}`}>
            {groqTestNote(groqSaved, groqTest)}
          </span>
        </div>
      </section>

      <section className="settings-section">
        <div className="settings-section__head">
          <h2 className="settings-section__title">{t("settings.system.title")}</h2>
        </div>
        <div className="settings-row">
          <div className="settings-row__text">
            <span className="settings-row__label">{t("settings.system.autostart")}</span>
            {autostartError && <span className="inline-note">{t("settings.system.autostartError")}</span>}
          </div>
          <div className="settings-row__control">
            <Toggle
              checked={autostartOn}
              disabled={autostartBusy}
              onChange={toggleAutostart}
              label={t("settings.system.autostart")}
            />
          </div>
        </div>
      </section>
    </div>
  );
}

function groqTestNote(saved: boolean, test: "idle" | "testing" | "ok" | "fail"): string {
  if (saved) return t("settings.groq.saved");
  if (test === "testing") return t("settings.groq.testing");
  if (test === "ok") return t("settings.groq.testOk");
  if (test === "fail") return t("settings.groq.testFail");
  return "";
}

function humanizeKey(key: string): string {
  const label = KEY_LABELS[key];
  if (label) return t(label);
  return key.replace(/([a-z0-9])([A-Z])/g, "$1 $2");
}

function Toggle({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: () => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className={`toggle${checked ? " is-on" : ""}`}
      disabled={disabled}
      onClick={onChange}
    >
      <span className="toggle__thumb" />
    </button>
  );
}

function RadioGroup<T extends string | number>({
  value,
  options,
  onChange,
  disabled,
  name,
}: {
  value: T;
  options: Array<{ id: T; label: StringKey }>;
  onChange: (v: T) => void;
  disabled?: boolean;
  name: string;
}) {
  return (
    <div className="radio-group">
      {options.map((opt) => (
        <label key={opt.id} className={`radio-option${value === opt.id ? " radio-option--active" : ""}`}>
          <input
            type="radio"
            name={name}
            checked={value === opt.id}
            disabled={disabled}
            onChange={() => onChange(opt.id)}
          />
          {t(opt.label)}
        </label>
      ))}
    </div>
  );
}

function StatusDot({ status }: { status: MeridianStatus }) {
  const cls =
    status === "online" ? "status-dot status-dot--ok" : status === "offline" ? "status-dot status-dot--fail" : "status-dot";
  return <span className={cls} aria-hidden />;
}
