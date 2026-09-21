import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useAuthStore } from "../../stores/authStore";
import { useToastStore } from "../../stores/toastStore";
import { getErrorMessage } from "../../lib/errors";
import {
  ACCOUNT_ICON_LIBRARY,
  accountDisplayName,
} from "../../lib/account";
import { AccountIcon } from "./AccountIcon";
import type { AccountEntry } from "../../lib/types";

interface EditAccountDialogProps {
  account: AccountEntry;
  onClose: () => void;
}

/**
 * Dialog for personalizing an account: set a display alias and pick an icon
 * from the built-in default icon library. Saving persists both fields.
 */
export function EditAccountDialog({ account, onClose }: EditAccountDialogProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const changeAccount = useAuthStore((s) => s.changeAccount);

  const defaultName = accountDisplayName(account);
  const [alias, setAlias] = useState(account.alias ?? "");
  const [selectedIcon, setSelectedIcon] = useState(account.icon ?? "");
  const [isSaving, setIsSaving] = useState(false);

  const handleSave = async () => {
    setIsSaving(true);
    try {
      // Empty values reset the field to the default on the backend.
      await changeAccount(
        account.homeAccountId,
        account.cloudType,
        alias.trim(),
        selectedIcon
      );
      addToast("success", t("accounts.editSaved"));
      onClose();
    } catch (err) {
      addToast("error", getErrorMessage(err));
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-full max-w-md rounded-lg bg-card p-6 shadow-lg border border-border space-y-4">
        <div className="flex items-center gap-3">
          <AccountIcon
            account={{ ...account, icon: selectedIcon }}
            className="h-8 w-8"
          />
          <h4 className="text-base font-semibold text-foreground">
            {t("accounts.editAccount")}
          </h4>
        </div>

        {/* Alias */}
        <div className="space-y-1.5">
          <label
            htmlFor="account-alias"
            className="text-sm font-medium text-foreground"
          >
            {t("accounts.aliasLabel")}
          </label>
          <input
            id="account-alias"
            value={alias}
            onChange={(e) => setAlias(e.target.value)}
            placeholder={defaultName || t("accounts.aliasPlaceholder")}
            maxLength={100}
            className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring"
          />
          <p className="text-xs text-muted-foreground">{t("accounts.aliasHint")}</p>
        </div>

        {/* Icon library */}
        <div className="space-y-1.5">
          <span className="text-sm font-medium text-foreground">
            {t("accounts.iconLabel")}
          </span>
          <div className="grid grid-cols-6 gap-2">
            {ACCOUNT_ICON_LIBRARY.map((def) => {
              const Icon = def.icon;
              const isSelected = selectedIcon === def.id;
              return (
                <button
                  key={def.id}
                  onClick={() => setSelectedIcon(def.id)}
                  title={def.id}
                  aria-label={def.id}
                  className={`flex h-10 w-10 items-center justify-center rounded-md border transition-colors ${
                    isSelected
                      ? "border-primary ring-2 ring-primary bg-accent"
                      : "border-border hover:bg-accent/50"
                  }`}
                >
                  <Icon className={`h-5 w-5 ${def.className}`} />
                </button>
              );
            })}
          </div>
          <p className="text-xs text-muted-foreground">{t("accounts.iconHint")}</p>
        </div>

        <div className="flex justify-end gap-2 pt-1">
          <button
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors"
          >
            {t("dialogs.cancel")}
          </button>
          <button
            onClick={handleSave}
            disabled={isSaving}
            className="rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
          >
            {t("dialogs.confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}
