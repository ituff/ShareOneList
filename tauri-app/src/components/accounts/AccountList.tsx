import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Pencil, Plus, Trash2 } from "lucide-react";
import { useAuthStore } from "../../stores/authStore";
import { LoginDialog } from "./LoginDialog";
import { EditAccountDialog } from "./EditAccountDialog";
import { AccountIcon } from "./AccountIcon";
import {
  accountDisplayName,
  accountKindLabelKey,
  resolveAccountKind,
} from "../../lib/account";
import type { AccountEntry, CloudEnvironment } from "../../lib/types";

interface AccountListProps {
  /** Called when a drive is selected (double-click on account). Optionally passes the full account entry. */
  onDriveSelect?: (driveId: string, driveName: string, cloudEnv: CloudEnvironment, account?: AccountEntry) => void;
}

/**
 * Displays the list of connected accounts with personalized alias/icon and
 * the account kind label (21Vianet / Global-Organization / Global-Personal).
 * Provides "Add Account", per-account edit and remove buttons.
 */
export function AccountList({ onDriveSelect }: AccountListProps) {
  const { t } = useTranslation();
  const { accounts, isLoaded, loadAccounts, refreshAccountTypes, removeAccount } = useAuthStore();
  const [showLoginDialog, setShowLoginDialog] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState<AccountEntry | null>(null);
  const [editing, setEditing] = useState<AccountEntry | null>(null);
  /** Guard so the background type refresh runs once per mount, not per render. */
  const typeRefreshStarted = useRef(false);

  // Load cached accounts from backend on mount
  useEffect(() => {
    if (!isLoaded) {
      loadAccounts();
    }
  }, [isLoaded, loadAccounts]);

  // Heal account types saved before driveType-based detection (or legacy
  // entries without a type) once after the accounts arrive.
  useEffect(() => {
    if (isLoaded && accounts.length > 0 && !typeRefreshStarted.current) {
      typeRefreshStarted.current = true;
      refreshAccountTypes();
    }
  }, [isLoaded, accounts, refreshAccountTypes]);

  const handleRemove = async (account: AccountEntry) => {
    await removeAccount(account.homeAccountId, account.cloudType);
    setConfirmRemove(null);
  };

  /** The account kind label. Three distinct kinds are shown — 21Vianet,
   * Global-Organization, Global-Personal — instead of just the cloud env. */
  const getAccountKindLabel = (account: AccountEntry): string => {
    const kind = resolveAccountKind(account.cloudType, account.accountType ?? null);
    return t(accountKindLabelKey(kind));
  };

  const getDisplayName = (account: AccountEntry): string => {
    return accountDisplayName(account) || getAccountKindLabel(account);
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
                <AccountIcon account={account} className="h-5 w-5" />
                <div>
                  <div className="text-sm font-medium text-foreground">
                    {getDisplayName(account)}
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {getAccountKindLabel(account)}
                  </div>
                </div>
              </div>
              <div className="flex items-center gap-1">
                <button
                  onClick={() => setEditing(account)}
                  className="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
                  aria-label={t("accounts.editAccount")}
                  title={t("accounts.editAccount")}
                >
                  <Pencil className="h-4 w-4" />
                </button>
                <button
                  onClick={() => setConfirmRemove(account)}
                  className="rounded-md p-1.5 text-muted-foreground hover:bg-destructive/10 hover:text-destructive transition-colors"
                  aria-label={t("accounts.removeAccount")}
                >
                  <Trash2 className="h-4 w-4" />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Login dialog */}
      {showLoginDialog && (
        <LoginDialog onClose={() => setShowLoginDialog(false)} />
      )}

      {/* Alias / icon edit dialog */}
      {editing && (
        <EditAccountDialog account={editing} onClose={() => setEditing(null)} />
      )}

      {/* Remove confirmation dialog */}
      {confirmRemove && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="w-full max-w-sm rounded-lg bg-card p-6 shadow-lg border border-border">
            <h4 className="text-base font-semibold text-foreground mb-2">
              {t("accounts.removeAccount")}
            </h4>
            <p className="text-sm text-muted-foreground mb-4">
              {getDisplayName(confirmRemove)} ({getAccountKindLabel(confirmRemove)})
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
