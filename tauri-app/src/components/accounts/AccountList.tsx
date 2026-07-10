import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2, Globe, Cloud } from "lucide-react";
import { useAuthStore } from "../../stores/authStore";
import { LoginDialog } from "./LoginDialog";
import type { AccountEntry, CloudEnvironment } from "../../lib/types";

interface AccountListProps {
  /** Called when a drive is selected (double-click on account). Optionally passes the full account entry. */
  onDriveSelect?: (driveId: string, driveName: string, cloudEnv: CloudEnvironment, account?: AccountEntry) => void;
}

/**
 * Displays the list of connected accounts with display name and cloud environment label.
 * Provides "Add Account" button and per-account remove button.
 */
export function AccountList({ onDriveSelect }: AccountListProps) {
  const { t } = useTranslation();
  const { accounts, isLoaded, loadAccounts, removeAccount } = useAuthStore();
  const [showLoginDialog, setShowLoginDialog] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState<AccountEntry | null>(null);

  // Load cached accounts from backend on mount
  useEffect(() => {
    if (!isLoaded) {
      loadAccounts();
    }
  }, [isLoaded, loadAccounts]);

  const handleRemove = async (account: AccountEntry) => {
    await removeAccount(account.homeAccountId, account.cloudType);
    setConfirmRemove(null);
  };

  const getCloudLabel = (cloudType: CloudEnvironment): string => {
    return cloudType === "global"
      ? t("accounts.cloudGlobal")
      : t("accounts.cloudChina");
  };

  const CloudIcon = ({ cloudType }: { cloudType: CloudEnvironment }) => {
    return cloudType === "global" ? (
      <Globe className="h-4 w-4 text-blue-500" />
    ) : (
      <Cloud className="h-4 w-4 text-orange-500" />
    );
  };

  return (
    <div className="space-y-4">
      {/* Header with Add Account button */}
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-semibold text-foreground">
          {t("files.title")}
        </h3>
        <button
          onClick={() => setShowLoginDialog(true)}
          className="inline-flex items-center gap-2 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
        >
          <Plus className="h-4 w-4" />
          {t("accounts.addAccount")}
        </button>
      </div>

      {/* Account list */}
      {!isLoaded ? (
        <div className="text-sm text-muted-foreground">Loading...</div>
      ) : accounts.length === 0 ? (
        <div className="rounded-lg border border-dashed border-border p-8 text-center">
          <p className="text-sm text-muted-foreground">
            {t("accounts.addAccount")}
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {accounts.map((account) => (
            <div
              key={account.homeAccountId}
              className="flex items-center justify-between rounded-lg border border-border bg-card px-4 py-3 hover:bg-accent/30 transition-colors cursor-pointer"
              onDoubleClick={() => {
                if (onDriveSelect) {
                  onDriveSelect(account.driveId, account.displayName, account.cloudType, account);
                }
              }}
            >
              <div className="flex items-center gap-3">
                <CloudIcon cloudType={account.cloudType} />
                <div>
                  <div className="text-sm font-medium text-foreground">
                    {account.displayName}
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {getCloudLabel(account.cloudType)}
                  </div>
                </div>
              </div>
              <button
                onClick={() => setConfirmRemove(account)}
                className="rounded-md p-1.5 text-muted-foreground hover:bg-destructive/10 hover:text-destructive transition-colors"
                aria-label={t("accounts.removeAccount")}
              >
                <Trash2 className="h-4 w-4" />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* Login dialog */}
      {showLoginDialog && (
        <LoginDialog onClose={() => setShowLoginDialog(false)} />
      )}

      {/* Remove confirmation dialog */}
      {confirmRemove && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-full max-w-sm rounded-lg bg-card p-6 shadow-lg border border-border">
            <h4 className="text-base font-semibold text-foreground mb-2">
              {t("accounts.removeAccount")}
            </h4>
            <p className="text-sm text-muted-foreground mb-4">
              {confirmRemove.displayName} ({getCloudLabel(confirmRemove.cloudType)})
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setConfirmRemove(null)}
                className="rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors"
              >
                {t("dialogs.cancel")}
              </button>
              <button
                onClick={() => handleRemove(confirmRemove)}
                className="rounded-md bg-destructive px-3 py-1.5 text-sm font-medium text-destructive-foreground hover:bg-destructive/90 transition-colors"
              >
                {t("dialogs.confirm")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
