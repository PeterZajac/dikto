import { useT } from "../../shared/i18n";
import { isMac } from "../../shared/platform";
import type { UpdateState } from "../../shared/update";
import "./update-banner.css";

interface UpdateBannerProps {
  state: UpdateState;
  onInstall: () => void;
  onDismiss: () => void;
}

export default function UpdateBanner({ state, onInstall, onDismiss }: UpdateBannerProps) {
  const t = useT();

  if (state.kind === "error") {
    return (
      <div className="update-banner update-banner--error" role="status">
        <span className="update-banner__title">{t("update.failed", { message: state.message })}</span>
      </div>
    );
  }

  const downloading = state.kind === "downloading";
  const version = state.kind === "available" || downloading ? state.version : "";

  return (
    <div className="update-banner" role="status">
      <div className="update-banner__text">
        <span className="update-banner__title">
          {state.kind === "ready" ? t("update.installing") : t("update.available.title", { version })}
        </span>
        {state.kind === "available" && state.notes.trim() !== "" && (
          <details className="update-banner__notes">
            <summary>{t("update.available.notes")}</summary>
            <pre className="update-banner__notes-body">{state.notes.trim()}</pre>
          </details>
        )}
        {isMac && state.kind === "available" && (
          <p className="update-banner__note">{t("update.permissionsNote")}</p>
        )}
      </div>

      <div className="update-banner__actions">
        <button
          type="button"
          className="update-banner__btn update-banner__btn--primary"
          disabled={state.kind !== "available"}
          onClick={onInstall}
        >
          {downloading
            ? t("update.downloading", { percent: state.percent })
            : state.kind === "ready"
              ? t("update.installing")
              : t("update.install")}
        </button>
        {state.kind === "available" && (
          <button type="button" className="update-banner__btn" onClick={onDismiss}>
            {t("update.later")}
          </button>
        )}
      </div>
    </div>
  );
}
