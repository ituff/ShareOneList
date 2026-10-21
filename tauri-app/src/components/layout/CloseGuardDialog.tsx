import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { Loader2, LogOut, MonitorDown } from "lucide-react";
import { CLOSE_GUARD_EVENT } from "../../lib/tauri";

/**
 * Close-guard dialog, shown on the backend's `close-guard-requested` event.
 * Only fires when close-to-tray is OFF and a mounted drive is active (with
 * the setting on, closing hides straight to tray, no dialog). The Rust
 * window-event hook has already prevented the close; here the user picks:
 * hide to tray (gateway keeps serving the mounts), unmount everything and
 * quit, or cancel.
 */
export function CloseGuardDialog() {
  const { t } = useTranslation();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [closing, setClosing] = useState(false);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    listen(CLOSE_GUARD_EVENT, () => {
      if (!disposed) setDialogOpen(true);
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const hideToTray = useCallback(async () => {
    setDialogOpen(false);
    await getCurrentWindow().hide();
  }, []);

  const unmountAndExit = useCallback(async () => {
    setClosing(true);
    // Backend unmounts every mounted drive, then exits.
    await invoke("quit_application_command");
    // The process exits; this state is just a worst-case fallback.
    setDialogOpen(false);
    setClosing(false);
  }, []);

  const cancelClose = useCallback(() => setDialogOpen(false), []);

  if (!dialogOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-full max-w-lg rounded-lg bg-card p-6 shadow-lg border border-border">
        <h3 className="text-lg font-semibold text-foreground mb-2">
          {t("closeGuard.title")}
        </h3>
        <p className="text-sm text-muted-foreground mb-4">
          {t("closeGuard.message")}
        </p>

        <div className="flex flex-nowrap items-center justify-end gap-2">
          <button
            onClick={cancelClose}
            disabled={closing}
            className="shrink-0 rounded-md px-3 py-2 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors disabled:opacity-50"
          >
            {t("dialogs.cancel")}
          </button>
          <button
            onClick={() => void unmountAndExit()}
            disabled={closing}
            className="inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-md border border-border px-3 py-2 text-sm font-medium text-foreground hover:bg-accent transition-colors disabled:opacity-50"
          >
            <LogOut className="h-4 w-4" />
            {t("closeGuard.unmountAndExit")}
          </button>
          <button
            onClick={() => void hideToTray()}
            disabled={closing}
            className="inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-md bg-primary px-3 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
          >
            <MonitorDown className="h-4 w-4" />
            {t("closeGuard.hideToTray")}
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
