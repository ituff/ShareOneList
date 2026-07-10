import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Globe, Cloud, Loader2 } from "lucide-react";
import { useAuthStore } from "../../stores/authStore";
import type { CloudEnvironment } from "../../lib/types";

interface LoginDialogProps {
  onClose: () => void;
}

/**
 * Modal dialog with cloud environment selector (Global / China).
 * Triggers the OAuth2 login flow via the authStore.
 */
export function LoginDialog({ onClose }: LoginDialogProps) {
  const { t } = useTranslation();
  const { addAccount, isLoggingIn, error, clearError } = useAuthStore();
  const [selectedEnv, setSelectedEnv] = useState<CloudEnvironment>("global");
  const [displayName, setDisplayName] = useState("");

  const handleLogin = async () => {
    clearError();
    try {
      await addAccount(selectedEnv, displayName.trim() || undefined);
      onClose();
    } catch {
      // Error is already set in the store
    }
  };

  const getErrorMessage = (err: string | null): string | null => {
    if (!err) return null;
    if (err === "DUPLICATE_ACCOUNT") {
      return t("accounts.alreadyConnected");
    }
    return t("errors.loginFailed");
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-full max-w-md rounded-lg bg-card p-6 shadow-lg border border-border">
        <h3 className="text-lg font-semibold text-foreground mb-4">
          {t("accounts.addAccount")}
        </h3>

        {/* Cloud environment selector */}
        <div className="space-y-3 mb-4">
          <label className="text-sm font-medium text-foreground">
            {t("accounts.cloudType")}
          </label>
          <div className="space-y-2">
            <label
              className={`flex items-center gap-3 rounded-lg border px-4 py-3 cursor-pointer transition-colors ${
                selectedEnv === "global"
                  ? "border-primary bg-primary/5"
                  : "border-border hover:bg-accent/30"
              }`}
            >
              <input
                type="radio"
                name="cloudEnv"
                value="global"
                checked={selectedEnv === "global"}
                onChange={() => setSelectedEnv("global")}
                className="sr-only"
              />
              <Globe className="h-5 w-5 text-blue-500" />
              <span className="text-sm font-medium text-foreground">
                {t("accounts.cloudGlobal")}
              </span>
            </label>

            <label
              className={`flex items-center gap-3 rounded-lg border px-4 py-3 cursor-pointer transition-colors ${
                selectedEnv === "china"
                  ? "border-primary bg-primary/5"
                  : "border-border hover:bg-accent/30"
              }`}
            >
              <input
                type="radio"
                name="cloudEnv"
                value="china"
                checked={selectedEnv === "china"}
                onChange={() => setSelectedEnv("china")}
                className="sr-only"
              />
              <Cloud className="h-5 w-5 text-orange-500" />
              <span className="text-sm font-medium text-foreground">
                {t("accounts.cloudChina")}
              </span>
            </label>
          </div>
        </div>

        {/* Optional display name */}
        <div className="mb-4">
          <label className="text-sm font-medium text-foreground block mb-1">
            {t("accounts.displayName")}
          </label>
          <input
            type="text"
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
            placeholder={t("accounts.displayNamePlaceholder")}
            className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring"
            disabled={isLoggingIn}
          />
        </div>

        {/* Error message */}
        {error && (
          <div className="mb-4 rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {getErrorMessage(error)}
          </div>
        )}

        {/* Actions */}
        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            disabled={isLoggingIn}
            className="rounded-md px-4 py-2 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors disabled:opacity-50"
          >
            {t("dialogs.cancel")}
          </button>
          <button
            onClick={handleLogin}
            disabled={isLoggingIn}
            className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
          >
            {isLoggingIn && <Loader2 className="h-4 w-4 animate-spin" />}
            {isLoggingIn ? "..." : t("dialogs.confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}
