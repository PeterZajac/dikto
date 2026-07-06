import type { ReactElement } from "react";

export type NavPage = "settings" | "history";

export interface FooterWarning {
  id: string;
  label: string;
  detail: string;
  actionLabel?: string;
  onAction?: () => void;
}

interface SidebarProps {
  page: NavPage;
  onNavigate: (page: NavPage) => void;
  version: string;
  warnings: FooterWarning[];
}

const NAV_ITEMS: Array<{ id: NavPage; label: string; icon: (props: { active: boolean }) => ReactElement }> = [
  { id: "settings", label: "Nastavenia", icon: SettingsIcon },
  { id: "history", label: "História", icon: HistoryIcon },
];

export default function Sidebar({ page, onNavigate, version, warnings }: SidebarProps) {
  return (
    <aside className="sidebar">
      <div className="sidebar__brand">
        <BrandMark />
        <span className="sidebar__brand-name">Wispr Flow</span>
      </div>

      <nav className="sidebar__nav">
        {NAV_ITEMS.map(({ id, label, icon: Icon }) => {
          const active = page === id;
          return (
            <button
              key={id}
              type="button"
              className={`sidebar__nav-item${active ? " is-active" : ""}`}
              aria-current={active ? "page" : undefined}
              onClick={() => onNavigate(id)}
            >
              <Icon active={active} />
              {label}
            </button>
          );
        })}
      </nav>

      <div className="sidebar__footer">
        {warnings.map((w) => (
          <WarningRow key={w.id} warning={w} />
        ))}
        <span className="sidebar__version">Local Wispr Flow{version ? ` v${version}` : ""}</span>
      </div>
    </aside>
  );
}

function WarningRow({ warning }: { warning: FooterWarning }) {
  return (
    <details className="warning-row">
      <summary className="warning-row__summary">
        <WarningIcon />
        {warning.label}
      </summary>
      <div className="warning-row__detail">
        <p>{warning.detail}</p>
        {warning.actionLabel && warning.onAction && (
          <button type="button" className="warning-row__action" onClick={warning.onAction}>
            {warning.actionLabel}
          </button>
        )}
      </div>
    </details>
  );
}

function BrandMark() {
  return (
    <div className="brand-mark" aria-hidden>
      <svg viewBox="0 0 20 20" width="16" height="16" fill="none">
        <rect x="2.8" y="7" width="2.4" height="6" rx="1.2" fill="currentColor" />
        <rect x="8.8" y="3.5" width="2.4" height="13" rx="1.2" fill="currentColor" />
        <rect x="14.8" y="7" width="2.4" height="6" rx="1.2" fill="currentColor" />
      </svg>
    </div>
  );
}

function SettingsIcon({ active }: { active: boolean }) {
  return (
    <svg viewBox="0 0 20 20" width="16" height="16" fill="none" aria-hidden>
      <circle cx="8" cy="5.5" r="1.7" fill={active ? "currentColor" : "none"} stroke="currentColor" strokeWidth="1.5" />
      <path d="M3 5.5h4M11.5 5.5H17" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <circle cx="13" cy="10" r="1.7" fill={active ? "currentColor" : "none"} stroke="currentColor" strokeWidth="1.5" />
      <path d="M3 10h8.5M14.7 10H17" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <circle cx="6.5" cy="14.5" r="1.7" fill={active ? "currentColor" : "none"} stroke="currentColor" strokeWidth="1.5" />
      <path d="M3 14.5h1.9M8.2 14.5H17" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  );
}

function HistoryIcon({ active }: { active: boolean }) {
  return (
    <svg viewBox="0 0 20 20" width="16" height="16" fill="none" aria-hidden>
      <path
        d="M10 3.5a6.5 6.5 0 1 1 -5.72 3.4"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        fill={active ? "var(--accent-subtle)" : "none"}
      />
      <path d="M2.6 3.4v3.9h3.9" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
      <path d="M10 6.7v3.4l2.4 1.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function WarningIcon() {
  return (
    <svg viewBox="0 0 20 20" width="13" height="13" fill="none" aria-hidden>
      <path
        d="M10 3.2 17.3 16H2.7L10 3.2Z"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinejoin="round"
      />
      <path d="M10 8v3.4" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
      <circle cx="10" cy="13.4" r="0.9" fill="currentColor" />
    </svg>
  );
}
