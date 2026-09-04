import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Sparkles, X } from "lucide-react";
import { checkUpdate, performUpdate } from "../../lib/tauri";
import { useUpdateStore } from "../../stores/updateStore";
import { useNotificationStore } from "../../stores/notificationStore";
import { formatFileSize } from "../../lib/formatters";

/**
 * Silent startup update check with a dismissible bubble at the top-right.
 * "Update now" downloads the installer (China mirror fallback, progress
 * reported by the backend) and opens it when done.
 */
export function UpdateBubble() {
  const { t } = useTranslation();
  const {
    info,
    dismissed,
    downloading,
    transferred,
    total,
    error,
    setInfo,
    dismiss,
    setDownloading,
    setProgress,
    setError,
  } = useUpdateStore();
  const pushNotification = useNotificationStore((s) => s.push);
  const checkStarted = useRef(false);

  // Silent check once per launch, 3s after startup; failures stay silent.
  useEffect(() => {
    if (checkStarted.current) return;
    checkStarted.current = true;
    const timer = setTimeout(async () => {
      try {
        const result = await checkUpdate();
        if (result) {
          setInfo(result);
          pushNotification({
            kind: "update",
            title: t("update.bubbleTitle", { version: result.version }),
            detail: result.changelog.split("\n").find((line) => line.trim()) ?? undefined,
          });
        }
      } catch {
        // Offline / GitHub unreachable: silently ignore.
      }
    }, 3000);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Listen for backend download progress.
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    listen<{ transferred: number; total: number }>(
      "update-download-progress",
      (event) => {
        setProgress(event.payload.transferred, event.payload.total);
      }
    ).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleUpdateNow = useCallback(async () => {
    if (!info || downloading) return;
    setDownloading(true);
    try {
      await performUpdate(info.version);
      setDownloading(false);
      pushNotification({
        kind: "download",
        title: t("update.installerReady"),
        detail: info.version,
      });
    } catch (err) {
      const message = String(err);
      setError(message);
      pushNotification({
        kind: "download-error",
        title: t("update.failed", { error: "" }).replace(/:\s*$/, ""),
        detail: message,
      });
    }
  }, [info, downloading, setDownloading, setError, pushNotification, t]);

  if (!info || dismissed) return null;

  const percent = total > 0 ? Math.min(100, Math.round((transferred / total) * 100)) : 0;

  return (
    <div className="fixed right-3 top-11 z-[85] w-80 rounded-lg border border-border bg-card p-3 shadow-lg">
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2">
          <Sparkles className="h-4 w-4 shrink-0 text-primary" />
          <span className="text-sm font-medium text-foreground">
            {t("update.bubbleTitle", { version: info.version })}
          </span>
        </div>
        <button
          onClick={dismiss}
          className="rounded p-0.5 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
          aria-label={t("update.later")}
          title={t("update.later")}
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </div>

      {info.changelog.trim() && (
        <p className="mt-2 line-clamp-3 whitespace-pre-wrap text-xs text-muted-foreground">
          {info.changelog}
        </p>
      )}

      {downloading ? (
        <div className="mt-3">
          <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
            <div
              className="h-full rounded-full bg-primary transition-all"
              style={{ width: `${percent}%` }}
            />
          </div>
          <p className="mt-1.5 text-xs text-muted-foreground">
            {t("update.downloading")}
            {total > 0
              ? ` ${formatFileSize(transferred)} / ${formatFileSize(total)}`
              : ""}
          </p>
        </div>
      ) : (
        <div className="mt-3 flex items-center justify-end gap-2">
          {error && (
            <span className="min-w-0 flex-1 truncate text-xs text-destructive" title={error}>
              {t("update.failed", { error })}
            </span>
          )}
          <button
            onClick={dismiss}
            className="rounded-md px-2.5 py-1.5 text-xs font-medium text-muted-foreground hover:bg-accent transition-colors"
          >
            {t("update.later")}
          </button>
          <button
            onClick={handleUpdateNow}
            className="rounded-md bg-primary px-2.5 py-1.5 text-xs font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
          >
            {t("update.updateNow")}
          </button>
        </div>
      )}
    </div>
  );
}
