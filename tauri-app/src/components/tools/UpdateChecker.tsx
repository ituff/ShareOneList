import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { getErrorMessage } from "../../lib/errors";

interface UpdateInfo {
  version: string;
  changelog: string;
  download_url: string;
}

type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "up-to-date" }
  | { status: "available"; info: UpdateInfo }
  | { status: "downloading" }
  | { status: "error"; message: string };

export function UpdateChecker() {
  const { t } = useTranslation();
  const [state, setState] = useState<UpdateState>({ status: "idle" });

  const handleCheck = async () => {
    setState({ status: "checking" });
    try {
      const result = await invoke<UpdateInfo | null>("check_update");
      if (result) {
        setState({ status: "available", info: result });
      } else {
        setState({ status: "up-to-date" });
      }
    } catch (err: unknown) {
      setState({ status: "error", message: getErrorMessage(err) });
    }
  };

  const handleDownload = async () => {
    if (state.status !== "available") return;
    const version = state.info.version;
    setState({ status: "downloading" });
    try {
      await invoke("perform_update", { version });
      // After opening the installer, reset to idle
      setState({ status: "idle" });
    } catch (err: unknown) {
      setState({ status: "error", message: getErrorMessage(err) });
    }
  };

  return (
    <section className="space-y-3 p-4 rounded-lg border border-border bg-card">
      <h3 className="text-lg font-semibold text-foreground">
        {t("settings.checkUpdate")}
      </h3>

      {/* Idle / Check Button */}
      {(state.status === "idle" || state.status === "up-to-date") && (
        <div className="space-y-2">
          <button
            onClick={handleCheck}
            className="px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm font-medium hover:bg-primary/90 transition-colors"
          >
            {t("settings.checkUpdate")}
          </button>
          {state.status === "up-to-date" && (
            <p className="text-sm text-muted-foreground">
              {t("update.upToDate")}
            </p>
          )}
        </div>
      )}

      {/* Checking / Loading */}
      {state.status === "checking" && (
        <div className="flex items-center gap-2">
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
          <span className="text-sm text-muted-foreground">
            {t("settings.checkUpdate")}...
          </span>
        </div>
      )}

      {/* Update Available */}
      {state.status === "available" && (
        <div className="space-y-3">
          <div className="p-3 rounded-md bg-muted/50 space-y-2">
            <p className="text-sm font-medium text-foreground">
              {t("update.available")}: v{state.info.version}
            </p>
            {state.info.changelog && (
              <div className="space-y-1">
                <p className="text-xs font-medium text-muted-foreground">
                  {t("update.changelog")}
                </p>
                <pre className="text-xs text-muted-foreground whitespace-pre-wrap max-h-40 overflow-y-auto">
                  {state.info.changelog}
                </pre>
              </div>
            )}
          </div>
          <div className="flex gap-2">
            <button
              onClick={handleDownload}
              className="px-4 py-2 rounded-md bg-primary text-primary-foreground text-sm font-medium hover:bg-primary/90 transition-colors"
            >
              {t("update.downloadAndInstall")}
            </button>
            <button
              onClick={() => setState({ status: "idle" })}
              className="px-4 py-2 rounded-md border border-input bg-background text-foreground text-sm hover:bg-accent transition-colors"
            >
              {t("dialogs.cancel")}
            </button>
          </div>
        </div>
      )}

      {/* Downloading */}
      {state.status === "downloading" && (
        <div className="flex items-center gap-2">
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
          <span className="text-sm text-muted-foreground">
            {t("update.downloading")}
          </span>
        </div>
      )}

      {/* Error */}
      {state.status === "error" && (
        <div className="space-y-2">
          <p className="text-sm text-destructive">
            {t("update.failed")}: {state.message}
          </p>
          <button
            onClick={handleCheck}
            className="px-4 py-2 rounded-md border border-input bg-background text-foreground text-sm hover:bg-accent transition-colors"
          >
            {t("errors.retryAction")}
          </button>
        </div>
      )}
    </section>
  );
}
