import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../../../shared/ipc";
import type { Dictation } from "../../../shared/ipc";
import { EVENT_HISTORY_CHANGED } from "../../../shared/events";
import { t, useLang, useT } from "../../../shared/i18n";
import "./history.css";

const SEARCH_DEBOUNCE_MS = 250;
const CLEAR_CONFIRM_MS = 3000;
const COPY_FEEDBACK_MS = 1500;
const EXPORT_FEEDBACK_MS = 4000;

export default function HistoryPage() {
  const [items, setItems] = useState<Dictation[] | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [query, setQuery] = useState("");
  const [searchTerm, setSearchTerm] = useState("");
  const [expandedIds, setExpandedIds] = useState<Set<number>>(new Set());
  const [copiedId, setCopiedId] = useState<number | null>(null);
  const [clearArmed, setClearArmed] = useState(false);
  const [retryingIds, setRetryingIds] = useState<Set<number>>(new Set());
  const [rowNotice, setRowNotice] = useState<{ id: number; text: string } | null>(null);
  const t = useT();

  const searchTermRef = useRef("");
  searchTermRef.current = searchTerm;

  const searchDebounceRef = useRef<number | undefined>(undefined);
  const copyTimerRef = useRef<number | undefined>(undefined);
  const clearArmedTimerRef = useRef<number | undefined>(undefined);
  const noticeTimerRef = useRef<number | undefined>(undefined);

  // ---- debounce the raw query into a committed search term ----
  useEffect(() => {
    window.clearTimeout(searchDebounceRef.current);
    searchDebounceRef.current = window.setTimeout(() => setSearchTerm(query.trim()), SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(searchDebounceRef.current);
  }, [query]);

  // ---- fetch on mount + whenever the committed search term changes ----
  useEffect(() => {
    let cancelled = false;
    api
      .historyList(searchTerm || undefined)
      .then((list) => {
        if (cancelled) return;
        setItems(list);
        setLoadError(false);
      })
      .catch(() => {
        if (!cancelled) setLoadError(true);
      });
    return () => {
      cancelled = true;
    };
  }, [searchTerm]);

  // ---- silent refetch: keeps the current list on screen until the new one lands ----
  const refetch = useCallback(() => {
    api
      .historyList(searchTermRef.current || undefined)
      .then((list) => {
        setItems(list);
        setLoadError(false);
      })
      .catch(() => {});
  }, []);

  // ---- refetch when the window regains focus ----
  useEffect(() => {
    window.addEventListener("focus", refetch);
    return () => window.removeEventListener("focus", refetch);
  }, [refetch]);

  // ---- refetch whenever the backend touches a row ----
  // Covers the whole lifecycle, not just a successful paste: a take shows up
  // here the moment recording stops, then updates as it succeeds or fails.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen(EVENT_HISTORY_CHANGED, () => refetch()).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refetch]);

  useEffect(
    () => () => {
      window.clearTimeout(copyTimerRef.current);
      window.clearTimeout(clearArmedTimerRef.current);
      window.clearTimeout(noticeTimerRef.current);
    },
    [],
  );

  const showNotice = useCallback((id: number, text: string) => {
    setRowNotice({ id, text });
    window.clearTimeout(noticeTimerRef.current);
    noticeTimerRef.current = window.setTimeout(() => setRowNotice(null), EXPORT_FEEDBACK_MS);
  }, []);

  const markRetrying = (id: number, active: boolean) =>
    setRetryingIds((prev) => {
      const next = new Set(prev);
      if (active) next.add(id);
      else next.delete(id);
      return next;
    });

  const handleRetry = (id: number) => {
    markRetrying(id, true);
    api
      .historyRetry(id)
      .catch((e) => showNotice(id, typeof e === "string" ? e : t("history.retryFailed")))
      .finally(() => {
        markRetrying(id, false);
        refetch();
      });
  };

  const handleExport = (id: number) => {
    api
      .historyExportAudio(id)
      .then((path) => showNotice(id, t("history.exported", { file: path.split(/[\\/]/).pop() ?? path })))
      .catch((e) => showNotice(id, typeof e === "string" ? e : t("history.exportFailed")));
  };

  const handleCopy = (id: number, text: string) => {
    navigator.clipboard
      .writeText(text)
      .then(() => {
        setCopiedId(id);
        window.clearTimeout(copyTimerRef.current);
        copyTimerRef.current = window.setTimeout(() => setCopiedId(null), COPY_FEEDBACK_MS);
      })
      .catch(() => {});
  };

  const toggleExpand = (id: number) => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleDelete = (id: number) => {
    setItems((prev) => prev && prev.filter((d) => d.id !== id));
    setExpandedIds((prev) => {
      if (!prev.has(id)) return prev;
      const next = new Set(prev);
      next.delete(id);
      return next;
    });
    api.historyDelete(id).catch(refetch);
  };

  const handleClearAllClick = () => {
    if (!clearArmed) {
      setClearArmed(true);
      window.clearTimeout(clearArmedTimerRef.current);
      clearArmedTimerRef.current = window.setTimeout(() => setClearArmed(false), CLEAR_CONFIRM_MS);
      return;
    }
    window.clearTimeout(clearArmedTimerRef.current);
    setClearArmed(false);
    setItems([]);
    setExpandedIds(new Set());
    api.historyClear().catch(refetch);
  };

  const count = items?.length ?? 0;
  const hasQuery = searchTerm.length > 0;

  return (
    <div className="history">
      <header className="history__header">
        <h1 className="history__title">{t("history.title")}</h1>
        <p className="history__subtitle">{t("history.subtitle")}</p>
      </header>

      <div className="history__toolbar">
        <div className="search-field">
          <SearchIcon />
          <input
            className="search-field__input"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("history.searchPlaceholder")}
            spellCheck={false}
          />
          {query && (
            <button
              type="button"
              className="search-field__clear"
              aria-label={t("history.clearSearch")}
              onClick={() => setQuery("")}
            >
              <ClearIcon />
            </button>
          )}
        </div>

        <div className="history__toolbar-right">
          {items !== null && (
            <span className="history__count">{formatCount(count)}</span>
          )}
          <button
            type="button"
            className={`history-btn history-btn--clear-all${clearArmed ? " is-armed" : ""}`}
            disabled={!items || items.length === 0}
            onClick={handleClearAllClick}
          >
            {clearArmed ? t("history.clearAllConfirm") : t("history.clearAll")}
          </button>
        </div>
      </div>

      {loadError && items === null && (
        <div className="history__banner">{t("history.loadError")}</div>
      )}

      {items === null && !loadError && <p className="history__loading">{t("history.loading")}</p>}

      {items !== null && items.length === 0 && hasQuery && (
        <EmptyState
          icon={<SearchIcon size={32} />}
          title={t("history.noResults.title")}
          hint={t("history.noResults.hint", { q: searchTerm })}
        />
      )}

      {items !== null && items.length === 0 && !hasQuery && (
        <EmptyState
          icon={<HistoryIcon />}
          title={t("history.empty.title")}
          hint={t("history.empty.hint")}
        />
      )}

      {items !== null && items.length > 0 && (
        <ul className="history-list">
          {items.map((item) => (
            <HistoryRow
              key={item.id}
              item={item}
              expanded={expandedIds.has(item.id)}
              copied={copiedId === item.id}
              retrying={retryingIds.has(item.id)}
              notice={rowNotice?.id === item.id ? rowNotice.text : null}
              searchTerm={searchTerm}
              onToggleExpand={toggleExpand}
              onCopy={handleCopy}
              onDelete={handleDelete}
              onRetry={handleRetry}
              onExport={handleExport}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

function HistoryRow({
  item,
  expanded,
  copied,
  retrying,
  notice,
  searchTerm,
  onToggleExpand,
  onCopy,
  onDelete,
  onRetry,
  onExport,
}: {
  item: Dictation;
  expanded: boolean;
  copied: boolean;
  retrying: boolean;
  notice: string | null;
  searchTerm: string;
  onToggleExpand: (id: number) => void;
  onCopy: (id: number, text: string) => void;
  onDelete: (id: number) => void;
  onRetry: (id: number) => void;
  onExport: (id: number) => void;
}) {
  const lang = useLang();
  const failed = item.status === "failed";
  const pending = item.status === "pending";
  const hasText = item.clean.length > 0;
  const hasAudio = item.audio_path !== null;

  return (
    <li
      className={`history-row history-row--${item.status}${expanded ? " history-row--expanded" : ""}`}
    >
      <div className="history-row__main">
        {hasText ? (
          <p className={`history-row__clean${expanded ? " history-row__clean--full" : ""}`}>
            {highlightMatch(item.clean, searchTerm)}
          </p>
        ) : (
          <p className="history-row__clean history-row__clean--placeholder">
            {pending ? t("history.row.transcribing") : t("history.row.noTranscript")}
          </p>
        )}

        <div className="history-row__meta">
          {failed && <span className="status-badge status-badge--failed">{t("history.row.failed")}</span>}
          {pending && <span className="status-badge status-badge--pending">{t("history.row.pending")}</span>}
          <span>{formatRelative(item.ts, lang)}</span>
          {item.language && <span className="lang-badge">{item.language.toUpperCase()}</span>}
          <span className="history-row__duration">{formatDuration(item.duration_ms)}</span>
          {hasAudio && <span className="history-row__audio" title={t("history.row.audioSaved")}>♪</span>}
        </div>

        {failed && item.error && <p className="history-row__error">{item.error}</p>}
        {notice && <p className="history-row__notice">{notice}</p>}

        {expanded && hasText && (
          <div className="history-row__raw">
            <span className="history-row__raw-label">{t("history.row.raw")}</span>
            <p className="history-row__raw-text">{item.raw}</p>
          </div>
        )}
      </div>

      <div className="history-row__actions">
        {(failed || pending) && hasAudio && (
          <button
            type="button"
            className="row-action row-action--primary"
            disabled={retrying}
            onClick={() => onRetry(item.id)}
          >
            {retrying ? t("history.row.transcribing") : t("history.row.retry")}
          </button>
        )}
        {hasAudio && (
          <button type="button" className="row-action" onClick={() => onExport(item.id)}>
            {t("history.row.export")}
          </button>
        )}
        {hasText && (
          <button type="button" className="row-action" onClick={() => onCopy(item.id, item.clean)}>
            {copied ? t("history.row.copied") : t("history.row.copy")}
          </button>
        )}
        {hasText && (
          <button type="button" className="row-action" onClick={() => onToggleExpand(item.id)}>
            {expanded ? t("history.row.collapse") : t("history.row.expand")}
          </button>
        )}
        <button type="button" className="row-action row-action--danger" onClick={() => onDelete(item.id)}>
          {t("history.row.delete")}
        </button>
      </div>
    </li>
  );
}

function EmptyState({ icon, title, hint }: { icon: ReactNode; title: string; hint: string }) {
  return (
    <div className="history-empty">
      <div className="history-empty__icon">{icon}</div>
      <h2 className="history-empty__title">{title}</h2>
      <p className="history-empty__hint">{hint}</p>
    </div>
  );
}

function formatCount(n: number): string {
  if (n === 1) return t("history.count.one", { n });
  if (n >= 2 && n <= 4) return t("history.count.few", { n });
  return t("history.count.many", { n });
}

function isSameDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

function formatRelative(ts: number, lang: "en" | "sk"): string {
  const now = Date.now();
  const diffMin = Math.floor((now - ts) / 60_000);
  if (diffMin < 1) return t("time.justNow");
  if (diffMin < 60) return t("time.minutesAgo", { n: diffMin });

  const date = new Date(ts);
  const today = new Date();
  if (isSameDay(date, today)) return t("time.hoursAgo", { n: Math.floor(diffMin / 60) });

  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (isSameDay(date, yesterday)) return t("time.yesterday");

  return date.toLocaleDateString(lang === "sk" ? "sk-SK" : "en-GB", { day: "numeric", month: "numeric", year: "numeric" });
}

function formatDuration(durationMs: number): string {
  const totalSec = Math.max(0, Math.round(durationMs / 1000));
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

function highlightMatch(text: string, term: string): ReactNode {
  if (!term) return text;
  const idx = text.toLowerCase().indexOf(term.toLowerCase());
  if (idx === -1) return text;
  return (
    <>
      {text.slice(0, idx)}
      <mark>{text.slice(idx, idx + term.length)}</mark>
      {text.slice(idx + term.length)}
    </>
  );
}

function SearchIcon({ size = 16 }: { size?: number }) {
  return (
    <svg viewBox="0 0 20 20" width={size} height={size} fill="none" aria-hidden>
      <circle cx="8.5" cy="8.5" r="5.5" stroke="currentColor" strokeWidth="1.4" />
      <path d="M12.6 12.6 17 17" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    </svg>
  );
}

function ClearIcon() {
  return (
    <svg viewBox="0 0 20 20" width="10" height="10" fill="none" aria-hidden>
      <path d="M4 4l12 12M16 4 4 16" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}

function HistoryIcon() {
  return (
    <svg viewBox="0 0 20 20" width="36" height="36" fill="none" aria-hidden>
      <path d="M10 3.5a6.5 6.5 0 1 1 -5.72 3.4" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
      <path d="M2.6 3.4v3.9h3.9" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
      <path d="M10 6.7v3.4l2.4 1.5" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}
