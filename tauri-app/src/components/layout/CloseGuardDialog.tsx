import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Minimize2, LogOut } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { webdavStatus, webdavUnmount } from "../../lib/tauri";

/**
 * Intercept window close while WebDAV mounts are OS-mounted: offer
 * minimize-to-taskbar (gateway keeps serving the mounts) vs. unmount-all-
 * then-quit. Closes normally when nothing is mounted. The close-requested
 * JS listener itself cancels the native close; we decide the outcome here.
 */
export function CloseGuardDialog() {
  const { t } = useTranslation();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [closing, setClosing] = useState(false);

  useEffect(() => {
    const appWindow = getCurrentWindow();
    let unlisten: (() => void) | null = null;
    let disposed = false;

    void appWindow.onCloseRequested(async (event) => {
      let hasMounted = false;
      try {
        const status = await webdavStatus();
        hasMounted = status.running && status.mounts.some((m) => m.mounted);
      } catch (err) {
        // A status failure must never trap the user inside the app.
        console.warn("[close-guard] status check failed:", err);
      }
      if (!hasMounted || disposed) return;
      event.preventDefault();
      setDialogOpen(true);
    }).then((un) => {
      if (disposed) un();
      else unlisten = un;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const minimizeToTaskbar = useCallback(async () => {
    setDialogOpen(false);
    await getCurrentWindow().minimize();
  }, []);

  const unmountAndExit = useCallback(async () => {
    setClosing(true);
    try {
      const status = await webdavStatus();
      const mounted = status.mounts.filter((m) => m.mounted);
      // Best-effort: one stuck mapping must not trap the user inside.
      for (const m of mounted) {
        try {
          await webdavUnmount(m.mountId);
        } catch (err) {
          console.warn("[close-guard] unmount failed:", m.mountId, err);
        }
      }
    } finally {
      await getCurrentWindow().destroy();
    }
  }, []);

  const cancelClose = useCallback(() => setDialogOpen(false), []);

  if (!dialogOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-full max-w-md rounded-lg bg-card p-6 shadow-lg border border-border">
        <h3 className="text-lg font-semibold text-foreground mb-2">
          {t("closeGuard.title")}
        </h3>
        <p className="text-sm text-muted-foreground mb-4">
          {t("closeGuard.message")}
        </p>

        <div className="flex flex-wrap justify-end gap-2">
          <button
            onClick={cancelClose}
            disabled={closing}
            className="rounded-md px-4 py-2 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors disabled:opacity-50"
          >
            {t("dialogs.cancel")}
          </button>
          <button
            onClick={() => void unmountAndExit()}
            disabled={closing}
            className="inline-flex items-center gap-2 rounded-md border border-border px-4 py-2 text-sm font-medium text-foreground hover:bg-accent transition-colors disabled:opacity-50"
          >
            <LogOut className="h-4 w-4" />
            {t("closeGuard.unmountAndExit")}
          </button>
          <button
            onClick={() => void minimizeToTaskbar()}
            disabled={closing}
            className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
          >
            <Minimize2 className="h-4 w-4" />
            {t("closeGuard.minimize")}
          </button>
        </div>

        {closing && (
          <p className="mt-3 flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t("closeGuard.unmounting")}
          </p>
        )}
      </div>
    </div>
  );
}
