import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../../../shared/ipc";
import type { Note } from "../../../shared/ipc";
import { EVENT_NOTES_CHANGED, type NotesChangedPayload } from "../../../shared/events";
import { formatRelative, hotkeyLabel, pluralCount } from "../../../shared/format";
import { useLang, useT } from "../../../shared/i18n";
import EmptyState from "../components/EmptyState";
import "./notes.css";

const SEARCH_DEBOUNCE_MS = 250;
/** History's search waits 250 ms; a write can afford a little more and still
 *  feel instant. The exposure is Quit from the tray — the main window's close
 *  is intercepted in Rust, so the page never unloads that way — and the worst
 *  case is ~400 ms of typing. That beats a SQLite write per keystroke; don't
 *  "fix" this by dropping the debounce. */
const AUTOSAVE_DEBOUNCE_MS = 400;
const DELETE_CONFIRM_MS = 3000;
const SAVED_FEEDBACK_MS = 1500;

type SaveState = "idle" | "saving" | "saved" | "error";

interface Draft {
  id: number;
  title: string;
  body: string;
}

export default function NotesPage({ hotkey }: { hotkey: string }) {
  const [items, setItems] = useState<Note[] | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [query, setQuery] = useState("");
  const [searchTerm, setSearchTerm] = useState("");
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [deleteArmed, setDeleteArmed] = useState(false);
  const [bodyFocused, setBodyFocused] = useState(false);
  const t = useT();
  const lang = useLang();

  const searchTermRef = useRef("");
  searchTermRef.current = searchTerm;
  const draftIdRef = useRef<number | null>(null);
  draftIdRef.current = draft?.id ?? null;

  const searchDebounceRef = useRef<number | undefined>(undefined);
  const savedFeedbackRef = useRef<number | undefined>(undefined);
  const deleteArmedRef = useRef<number | undefined>(undefined);
  const titleInputRef = useRef<HTMLInputElement | null>(null);
  // The input only exists after the render that follows notes_create, so the
  // focus has to wait for it.
  const focusTitleRef = useRef(false);

  // The pending patch lives in a ref, not in state, so a flush triggered by
  // blur or a note switch sends the latest keystroke even before React has
  // re-rendered.
  const pendingRef = useRef<{ id: number; title?: string; body?: string } | null>(null);
  const autosaveRef = useRef<number | undefined>(undefined);

  const flush = useCallback(() => {
    window.clearTimeout(autosaveRef.current);
    const patch = pendingRef.current;
    pendingRef.current = null;
    if (!patch) return;
    setSaveState("saving");
    api
      .notesUpdate(patch.id, { title: patch.title, body: patch.body })
      .then(() => {
        setSaveState("saved");
        window.clearTimeout(savedFeedbackRef.current);
        savedFeedbackRef.current = window.setTimeout(() => setSaveState("idle"), SAVED_FEEDBACK_MS);
      })
      .catch(() => setSaveState("error"));
  }, []);

  const queuePatch = useCallback(
    (id: number, patch: { title?: string; body?: string }) => {
      pendingRef.current = { ...(pendingRef.current ?? { id }), id, ...patch };
      window.clearTimeout(autosaveRef.current);
      autosaveRef.current = window.setTimeout(flush, AUTOSAVE_DEBOUNCE_MS);
    },
    [flush],
  );

  // ---- debounce the raw query into a committed search term ----
  useEffect(() => {
    window.clearTimeout(searchDebounceRef.current);
    searchDebounceRef.current = window.setTimeout(() => setSearchTerm(query.trim()), SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(searchDebounceRef.current);
  }, [query]);

  useEffect(() => {
    let cancelled = false;
    api
      .notesList(searchTerm || undefined)
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

  const refetch = useCallback(() => {
    api
      .notesList(searchTermRef.current || undefined)
      .then((list) => {
        setItems(list);
        setLoadError(false);
      })
      .catch(() => {});
  }, []);

  // The list always refreshes; the open editor's buffer is never replaced from
  // an event its own autosave caused.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen<NotesChangedPayload>(EVENT_NOTES_CHANGED, (event) => {
      refetch();
      const { id, reason } = event.payload;
      if (reason === "delete" && id === draftIdRef.current) setDraft(null);
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refetch]);

  // Quit from the tray is the one path that unloads the page.
  useEffect(() => {
    window.addEventListener("pagehide", flush);
    return () => window.removeEventListener("pagehide", flush);
  }, [flush]);

  // Leaving the Notes tab unmounts the page — flush what is still queued.
  useEffect(
    () => () => {
      flush();
      window.clearTimeout(savedFeedbackRef.current);
      window.clearTimeout(deleteArmedRef.current);
    },
    [flush],
  );

  useEffect(() => {
    if (!focusTitleRef.current) return;
    focusTitleRef.current = false;
    titleInputRef.current?.focus();
  }, [draft?.id]);

  const selectNote = (note: Note) => {
    if (note.id === draft?.id) return;
    flush();
    setSaveState("idle");
    setDeleteArmed(false);
    setDraft({ id: note.id, title: note.title, body: note.body });
  };

  const handleNew = () => {
    flush();
    api
      .notesCreate()
      .then((note) => {
        setSaveState("idle");
        setDeleteArmed(false);
        focusTitleRef.current = true;
        setDraft({ id: note.id, title: note.title, body: note.body });
        refetch();
      })
      .catch(() => setSaveState("error"));
  };

  const handleDelete = () => {
    if (!draft) return;
    if (!deleteArmed) {
      setDeleteArmed(true);
      window.clearTimeout(deleteArmedRef.current);
      deleteArmedRef.current = window.setTimeout(() => setDeleteArmed(false), DELETE_CONFIRM_MS);
      return;
    }
    window.clearTimeout(deleteArmedRef.current);
    window.clearTimeout(autosaveRef.current);
    pendingRef.current = null;
    const id = draft.id;
    setDeleteArmed(false);
    setDraft(null);
    setItems((prev) => prev && prev.filter((n) => n.id !== id));
    api.notesDelete(id).catch(refetch);
  };

  const count = items?.length ?? 0;
  const hasQuery = searchTerm.length > 0;

  return (
    <div className="notes">
      <header className="notes__header">
        <h1 className="notes__title">{t("notes.title")}</h1>
        <p className="notes__subtitle">{t("notes.subtitle")}</p>
      </header>

      <div className="notes__toolbar">
        <div className="search-field">
          <SearchIcon />
          <input
            className="search-field__input"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("notes.searchPlaceholder")}
            spellCheck={false}
          />
          {query && (
            <button
              type="button"
              className="search-field__clear"
              aria-label={t("notes.clearSearch")}
              onClick={() => setQuery("")}
            >
              <ClearIcon />
            </button>
          )}
        </div>

        <div className="notes__toolbar-right">
          {items !== null && <span className="notes__count">{pluralCount(count, "notes.count")}</span>}
          <button type="button" className="notes-btn notes-btn--new" onClick={handleNew}>
            {t("notes.new")}
          </button>
        </div>
      </div>

      {loadError && items === null && <div className="notes__banner">{t("notes.loadError")}</div>}

      {items !== null && items.length === 0 && hasQuery && (
        <EmptyState
          icon={<SearchIcon size={32} />}
          title={t("notes.noResults.title")}
          hint={t("notes.noResults.hint", { q: searchTerm })}
        />
      )}

      {items !== null && items.length === 0 && !hasQuery && (
        <EmptyState icon={<NotesGlyph />} title={t("notes.empty.title")} hint={t("notes.empty.hint")} />
      )}

      {items !== null && items.length > 0 && (
        <div className="notes__split">
          <ul className="notes-list">
            {items.map((note) => (
              <li key={note.id}>
                <button
                  type="button"
                  className={`note-row${note.id === draft?.id ? " is-active" : ""}`}
                  aria-current={note.id === draft?.id ? "true" : undefined}
                  onClick={() => selectNote(note)}
                >
                  <span className="note-row__title">
                    {(note.id === draft?.id ? draft.title : note.title) || t("notes.untitled")}
                  </span>
                  <span className="note-row__preview">
                    {(note.id === draft?.id ? draft.body : note.body).trim().split("\n")[0]}
                  </span>
                  <span className="note-row__time">{formatRelative(note.updated_at, lang)}</span>
                </button>
              </li>
            ))}
          </ul>

          {draft === null ? (
            <div className="page">
              <h2 className="page__title">{t("notes.none.title")}</h2>
              <p className="page__hint">{t("notes.none.hint")}</p>
            </div>
          ) : (
            <div className="note-editor">
              <input
                ref={titleInputRef}
                className="note-editor__title"
                value={draft.title}
                placeholder={t("notes.titlePlaceholder")}
                aria-label={t("notes.titlePlaceholder")}
                onChange={(e) => {
                  const title = e.target.value;
                  setDraft({ ...draft, title });
                  queuePatch(draft.id, { title });
                }}
                onBlur={flush}
              />
              <textarea
                className="note-editor__body"
                value={draft.body}
                placeholder={t("notes.bodyPlaceholder")}
                aria-label={t("notes.bodyPlaceholder")}
                onChange={(e) => {
                  const body = e.target.value;
                  setDraft({ ...draft, body });
                  queuePatch(draft.id, { body });
                }}
                onFocus={() => setBodyFocused(true)}
                onBlur={() => {
                  setBodyFocused(false);
                  flush();
                }}
              />
              <div className="note-editor__footer">
                {bodyFocused && (
                  <span className="note-editor__pill">
                    {t("notes.dictationHint", { key: hotkeyLabel(hotkey) })}
                  </span>
                )}
                <span className="note-editor__save" data-state={saveState}>
                  {saveState === "saving" && t("notes.saving")}
                  {saveState === "saved" && t("notes.saved")}
                  {saveState === "error" && t("notes.saveError")}
                </span>
                <button
                  type="button"
                  className={`notes-btn notes-btn--delete${deleteArmed ? " is-armed" : ""}`}
                  onClick={handleDelete}
                >
                  {deleteArmed ? t("notes.deleteConfirm") : t("notes.delete")}
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
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

function NotesGlyph() {
  return (
    <svg viewBox="0 0 20 20" width="36" height="36" fill="none" aria-hidden>
      <path
        d="M5 2.8h7.2L16 6.6V17a1.2 1.2 0 0 1-1.2 1.2H5A1.2 1.2 0 0 1 3.8 17V4A1.2 1.2 0 0 1 5 2.8Z"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinejoin="round"
      />
      <path d="M11.8 3v3.8H15.8" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" />
      <path d="M6.6 10.4h6.8M6.6 13.4h4.4" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
    </svg>
  );
}
