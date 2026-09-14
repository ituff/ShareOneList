import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { getErrorMessage } from "../../lib/errors";
import { useSettingsStore, type UpdateChannel } from "../../stores/settingsStore";

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

/** The About tab's channel toggle, version row with the update button and
 * inline states (changelog / download / error) rendered beneath the row. */
export function UpdateChecker() {
  const { t } = useTranslation();
  const updateChannel = useSettingsStore((s) => s.updateChannel);
  const setUpdateChannel = useSettingsStore((s) => s.setUpdateChannel);
  const [state, setState] = useState<UpdateState>({ status: "idle" });
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    getVersion().then(setVersion).catch(() => setVersion(null));
  }, []);

  const handleCheck = async () => {
    setState({ status: "checking" });
    try {
      const result = await invoke<UpdateInfo | null>("check_update", {
        channel: updateChannel,
      });
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
      await invoke("perform_update", { version, channel: updateChannel });
      // After opening the installer, reset to idle
      setState({ status: "idle" });
    } catch (err: unknown) {
      setState({ status: "error", message: getErrorMessage(err) });
    }
  };

  // Switching channels invalidates the previous check result.
  const handleChannelChange = (channel: UpdateChannel) => {
    setUpdateChannel(channel);
    if (state.status !== "idle") setState({ status: "idle" });
  };

  const control = () => {
    switch (state.status) {
      case "checking":
      case "downloading":
        return (
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
        );
      case "up-to-date":
        return <span className="text-xs text-muted-foreground">{t("update.upToDate")}</span>;
      default:
        return (
          <button
            onClick={handleCheck}
            className="rounded-md bg-primary px-3 py-1 text-xs font-medium text-primary-foreground hover:bg-primary/90"
          >
            {t("settings.checkUpdate")}
          </button>
        );
    }
  };

  const channelToggle = () => (
    <div className="flex items-center rounded-md border border-border bg-background p-0.5">
      {(
        [
          { value: "stable", label: t("settings.channelStable") },
          { value: "beta", label: t("settings.channelBeta") },
        ] as const
      ).map((option) => (
        <button
          key={option.value}
          onClick={() => handleChannelChange(option.value)}
          className={`rounded px-2.5 py-1 text-xs font-medium transition-colors ${
            updateChannel === option.value
              ? "bg-primary text-primary-foreground"
              : "text-muted-foreground hover:text-foreground"
          }`}
          aria-pressed={updateChannel === option.value}
        >
          {option.label}
        </button>
      ))}
    </div>
  );

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between rounded-md bg-muted/40 px-3 py-2">
        <span className="text-sm text-muted-foreground">{t("settings.updateChannel")}</span>
        {channelToggle()}
      </div>

      <div className="flex items-center justify-between rounded-md bg-muted/40 px-3 py-2">
        <span className="text-sm text-muted-foreground">{t("settings.version")}</span>
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-foreground">{version ?? "..."}</span>
          {control()}
        </div>
      </div>

      {state.status === "available" && (
        <div className="space-y-2 rounded-md bg-muted/50 p-3">
          <p className="text-sm font-medium text-foreground">
            {t("update.available")}: v{state.info.version}
          </p>
          {state.info.changelog && (
            <div className="space-y-1">
              <p className="text-xs font-medium text-muted-foreground">
                {t("update.changelog")}
              </p>
              <pre className="max-h-40 overflow-y-auto whitespace-pre-wrap text-xs text-muted-foreground">
                {state.info.changelog}
              </pre>
            </div>
          )}
          <div className="flex gap-2">
            <button
              onClick={handleDownload}
              className="rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90"
            >
              {t("update.downloadAndInstall")}
            </button>
            <button
              onClick={() => setState({ status: "idle" })}
              className="rounded-md border border-input bg-background px-4 py-2 text-sm text-foreground hover:bg-accent"
            >
              {t("dialogs.cancel")}
            </button>
          </div>
        </div>
      )}

      {state.status === "downloading" && (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
          {t("update.downloading")}
        </div>
      )}

      {state.status === "error" && (
        <div className="space-y-2">
          <p className="text-sm text-destructive">
            {t("update.failed")}: {state.message}
          </p>
          <button
            onClick={handleCheck}
            className="rounded-md border border-input bg-background px-4 py-2 text-sm text-foreground hover:bg-accent"
          >
            {t("errors.retryAction")}
          </button>
        </div>
      )}
    </div>
  );
}
