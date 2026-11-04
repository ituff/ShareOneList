import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Folder, FolderPlus, Loader2 } from "lucide-react";
import type {
  AccountEntry,
  AutoBackupTarget,
  BreadcrumbItem,
  DriveItem,
} from "../../lib/types";
import { createFolder, listFiles } from "../../lib/tauri";
import { accountDisplayName } from "../../lib/account";
import { getErrorMessage } from "../../lib/errors";
import { validateFileName } from "../../lib/validators";
import { useAuthStore } from "../../stores/authStore";
import { FolderBreadcrumb } from "../files/FolderBreadcrumb";

interface CloudBackupFolderPickerDialogProps {
  /** Currently configured target, used to preselect its account. */
  initial: AutoBackupTarget | null;
  onClose: () => void;
  /** Called with the chosen folder; rejects keep the dialog open. */
  onConfirm: (target: AutoBackupTarget) => Promise<void>;
}

/** Composite key identifying an account across both clouds. */
function accountKey(account: Pick<AccountEntry, "homeAccountId" | "cloudType">): string {
  return `${account.homeAccountId}:${account.cloudType}`;
}

/**
 * Modal picker for the automatic backup destination: pick one of the
 * signed-in accounts, then browse (or create) a folder inside that
 * account's OneDrive. The chosen folder is returned as an `AutoBackupTarget`.
 */
export function CloudBackupFolderPickerDialog({
  initial,
  onClose,
  onConfirm,
}: CloudBackupFolderPickerDialogProps) {
  const { t } = useTranslation();
  const accounts = useAuthStore((s) => s.accounts);

  const [selectedKey, setSelectedKey] = useState<string>(() => {
    if (initial) {
      const match = accounts.find(
        (a) => a.homeAccountId === initial.homeAccountId && a.cloudType === initial.cloudEnv
      );
      if (match) return accountKey(match);
    }
    return accounts.length > 0 ? accountKey(accounts[0]) : "";
  });
  const account = useMemo(
    () => accounts.find((a) => accountKey(a) === selectedKey) ?? null,
    [accounts, selectedKey]
  );

  const [folderId, setFolderId] = useState("root");
  const [breadcrumbs, setBreadcrumbs] = useState<BreadcrumbItem[]>([]);
  const [folders, setFolders] = useState<DriveItem[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [isCreating, setIsCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [isCreatingBusy, setIsCreatingBusy] = useState(false);
  const [isSaving, setIsSaving] = useState(false);

  const busy = isSaving || isCreatingBusy;

  const loadFolders = useCallback(async () => {
    if (!account) {
      setFolders([]);
      return;
    }
    setIsLoading(true);
    setLoadError(null);
    try {
      const items = await listFiles(account.driveId, folderId, account.cloudType);
      setFolders(
        items
          .filter((item) => item.isFolder)
          .sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }))
      );
    } catch (err) {
      setFolders([]);
      setLoadError(getErrorMessage(err));
    } finally {
      setIsLoading(false);
    }
  }, [account, folderId]);

  useEffect(() => {
    loadFolders();
  }, [loadFolders]);

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) onClose();
    };
    document.addEventListener("keydown", handleEscape);
    return () => document.removeEventListener("keydown", handleEscape);
  }, [busy, onClose]);

  const resetToRoot = () => {
    setFolderId("root");
    setBreadcrumbs([]);
    setIsCreating(false);
    setNewName("");
  };

  const handleAccountChange = (key: string) => {
    setSelectedKey(key);
    resetToRoot();
  };

  const enterFolder = (item: Pick<DriveItem, "id" | "name">) => {
    setBreadcrumbs((prev) => [...prev, { id: item.id, name: item.name }]);
    setFolderId(item.id);
    setIsCreating(false);
    setNewName("");
  };

  const navigateIndex = (index: number) => {
    const next = breadcrumbs.slice(0, index + 1);
    setBreadcrumbs(next);
    setFolderId(next[next.length - 1].id);
  };

  const folderPath = "/" + breadcrumbs.map((b) => b.name).join("/");

  const nameValidation =
    newName.length > 0 ? validateFileName(newName) : { valid: false, error: undefined };

  const handleCreate = async () => {
    if (!account || !nameValidation.valid) return;
    setIsCreatingBusy(true);
    try {
      const created = await createFolder(account.driveId, folderId, newName, account.cloudType);
      // Jump straight into the new folder — it is almost always the
      // destination the user wants to pick next.
      enterFolder(created);
    } catch (err) {
      setLoadError(getErrorMessage(err));
    } finally {
      setIsCreatingBusy(false);
    }
  };

  const handleConfirm = async () => {
    if (!account) return;
    setIsSaving(true);
    try {
      await onConfirm({
        homeAccountId: account.homeAccountId,
        cloudEnv: account.cloudType,
        driveId: account.driveId,
        folderId,
        folderPath,
      });
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={busy ? undefined : onClose}
      role="dialog"
      aria-modal="true"
      aria-labelledby="backup-folder-picker-title"
    >
      <div
        className="flex h-[520px] w-full max-w-xl flex-col rounded-lg border border-border bg-card p-6 shadow-lg"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="backup-folder-picker-title" className="mb-4 text-lg font-semibold text-foreground">
          {t("backup.picker.title")}
        </h3>

        {accounts.length === 0 ? (
          <p className="text-sm text-muted-foreground">{t("backup.picker.noAccounts")}</p>
        ) : (
          <>
            <label className="mb-1 block text-sm font-medium text-foreground">
              {t("backup.picker.account")}
            </label>
            <select
              value={selectedKey}
              onChange={(e) => handleAccountChange(e.target.value)}
              disabled={busy}
              className="mb-3 w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-ring disabled:opacity-50"
              aria-label={t("backup.picker.account")}
            >
              {accounts.map((a) => (
                <option key={accountKey(a)} value={accountKey(a)}>
                  {accountDisplayName(a)}
                  {a.cloudType === "china" ? ` (${t("webdavMount.envChina")})` : ""}
                </option>
              ))}
            </select>

            <div className="mb-2 flex items-center gap-2">
              <div className="min-w-0 flex-1">
                <FolderBreadcrumb
                  driveName={account ? accountDisplayName(account) : ""}
                  breadcrumbs={breadcrumbs}
                  onNavigateRoot={resetToRoot}
                  onNavigateIndex={navigateIndex}
                />
              </div>
              <button
                onClick={() => setIsCreating((v) => !v)}
                disabled={busy || !account}
                className="inline-flex shrink-0 items-center gap-1.5 rounded-md border border-border bg-background px-2.5 py-1.5 text-xs font-medium text-foreground hover:bg-accent transition-colors disabled:opacity-50"
              >
                <FolderPlus className="h-3.5 w-3.5" />
                {t("backup.picker.newFolder")}
              </button>
            </div>

            {isCreating && (
              <div className="mb-2 flex items-center gap-2">
                <input
                  type="text"
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && nameValidation.valid && !busy) handleCreate();
                  }}
                  placeholder={t("backup.picker.newFolderPlaceholder")}
                  disabled={busy}
                  autoFocus
                  className="min-w-0 flex-1 rounded-md border border-border bg-background px-3 py-1.5 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring"
                  aria-label={t("backup.picker.newFolderPlaceholder")}
                />
                <button
                  onClick={handleCreate}
                  disabled={busy || !nameValidation.valid}
                  className="inline-flex shrink-0 items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
                >
                  {isCreatingBusy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                  {t("dialogs.create")}
                </button>
              </div>
            )}
            {isCreating && newName.length > 0 && !nameValidation.valid && nameValidation.error && (
              <p className="mb-2 text-xs text-destructive">{nameValidation.error}</p>
            )}

            <div className="min-h-0 flex-1 overflow-y-auto rounded-md border border-border bg-background">
              {isLoading ? (
                <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground">
                  <Loader2 className="h-4 w-4 animate-spin" />
                  {t("backup.picker.loading")}
                </div>
              ) : loadError ? (
                <div className="p-3 text-sm text-destructive">{loadError}</div>
              ) : folders.length === 0 ? (
                <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
                  {t("backup.picker.empty")}
                </div>
              ) : (
                <ul className="py-1">
                  {folders.map((item) => (
                    <li key={item.id}>
                      <button
                        onClick={() => enterFolder(item)}
                        disabled={busy}
                        className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm text-foreground hover:bg-accent transition-colors disabled:opacity-50"
                        title={item.name}
                      >
                        <Folder className="h-4 w-4 shrink-0 text-muted-foreground" />
                        <span className="truncate">{item.name}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>

            <p className="mt-3 truncate text-xs text-muted-foreground" title={folderPath}>
              {t("backup.picker.currentPath")}: {folderPath}
            </p>
          </>
        )}

        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onClose}
            disabled={busy}
            className="rounded-md px-4 py-2 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors disabled:opacity-50"
          >
            {t("dialogs.cancel")}
          </button>
          <button
            onClick={handleConfirm}
            disabled={busy || !account || isLoading}
            className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
          >
            {isSaving && <Loader2 className="h-4 w-4 animate-spin" />}
            {t("backup.picker.useFolder")}
          </button>
        </div>
      </div>
    </div>
  );
}
