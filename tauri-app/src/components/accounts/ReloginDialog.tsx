import { useTranslation } from "react-i18next";
import { Loader2, RotateCw } from "lucide-react";
import { useAuthStore } from "../../stores/authStore";
import { useTabStore } from "../../stores/tabStore";
import { useToastStore } from "../../stores/toastStore";
import { resumeDownload } from "../../lib/tauri";
import { getErrorMessage } from "../../lib/errors";

export function ReloginDialog() {
  const { t } = useTranslation();
  const pending = useAuthStore((s) => s.pendingRelogin);
  const isLoggingIn = useAuthStore((s) => s.isLoggingIn);
  const error = useAuthStore((s) => s.error);
  const addToast = useToastStore((s) => s.addToast);
  const reloginAccount = useAuthStore((s) => s.reloginAccount);
  const clearPendingRelogin = useAuthStore((s) => s.clearPendingRelogin);

  if (!pending) return null;

  const handleConfirm = async () => {
    try {
      await reloginAccount(pending.cloudEnv);
      if (pending.taskId) {
        try {
          await resumeDownload(pending.cloudEnv, pending.taskId);
        } catch (err) {
          addToast("error", getErrorMessage(err));
          clearPendingRelogin();
          return;
        }
      }
      if (pending.tabId) {
        await useTabStore.getState().loadFolder(pending.tabId, pending.folderId ?? "root");
      }
      clearPendingRelogin();
    } catch {
      // The store exposes the error inside the dialog so the user can retry.
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-full max-w-md rounded-lg bg-card p-6 shadow-lg border border-border">
        <h3 className="text-lg font-semibold text-foreground mb-2">
          {t("accounts.reLoginTitle")}
        </h3>
        <p className="text-sm text-muted-foreground mb-4">
          {t("accounts.reLoginMessage")}
        </p>

        {error && (
          <div className="mb-4 rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}

        <div className="flex justify-end gap-2">
          <button
            onClick={clearPendingRelogin}
            disabled={isLoggingIn}
            className="rounded-md px-4 py-2 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors disabled:opacity-50"
          >
            {t("dialogs.cancel")}
          </button>
          <button
            onClick={handleConfirm}
            disabled={isLoggingIn}
            className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
          >
            {isLoggingIn ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <RotateCw className="h-4 w-4" />
            )}
            {isLoggingIn ? "..." : t("accounts.reLoginAction")}
          </button>
        </div>
      </div>
    </div>
  );
}
