import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  Cloud,
  Download,
  FolderOpen,
  KeyRound,
  ShieldCheck,
  Upload,
} from "lucide-react";
import {
  exportConfig,
  getBackupPasswordState,
  importConfig,
  inspectBackup,
  setAutoBackupTarget,
  setBackupPassword,
} from "../../lib/tauri";
import type { ImportSummary, BackupInspectInfo } from "../../lib/tauri";
import type { AutoBackupTarget, BackupItem } from "../../lib/types";
import { accountDisplayName } from "../../lib/account";
import { getErrorMessage } from "../../lib/errors";
import { useSettingsStore } from "../../stores/settingsStore";
import { useAuthStore } from "../../stores/authStore";
import { useToastStore } from "../../stores/toastStore";
import { CloudBackupFolderPickerDialog } from "./CloudBackupFolderPickerDialog";

function defaultBackupName(): string {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `ShareOneList-backup-${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(
    now.getDate()
  )}-${pad(now.getHours())}${pad(now.getMinutes())}.json`;
}

/** The backupable optional sections, in display order. */
const BACKUP_ITEMS: BackupItem[] = [
  "tokens",
  "llm_config",
  "llm_keys",
  "chat_history",
  "catalog",
  "webdav_mounts",
  "download_tasks",
];

/** The "Backup & restore" tab: manual export/import, selectable backup
 * contents with password encryption, and the automatic cloud backup. */
export function BackupSettings() {
  const { t } = useTranslation();
  const autoBackupTarget = useSettingsStore((s) => s.autoBackupTarget);
  const autoBackupItems = useSettingsStore((s) => s.autoBackupItems);
  const setAutoBackupItems = useSettingsStore((s) => s.setAutoBackupItems);
  const loadConfig = useSettingsStore((s) => s.loadConfig);
  const accounts = useAuthStore((s) => s.accounts);
  const addToast = useToastStore((s) => s.addToast);
  const [pendingImport, setPendingImport] = useState<{
    path: string;
    info: BackupInspectInfo;
    selected: BackupItem[];
  } | null>(null);
  const [importPassword, setImportPassword] = useState("");
  const [pickerOpen, setPickerOpen] = useState(false);

  // Backup password state.
  const [passwordSet, setPasswordSet] = useState(false);
  const [passwordEditing, setPasswordEditing] = useState(false);
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [savingPassword, setSavingPassword] = useState(false);

  useEffect(() => {
    getBackupPasswordState()
      .then((state) => setPasswordSet(state.isSet))
      .catch(() => setPasswordSet(false));
  }, []);

  const targetAccount = autoBackupTarget
    ? accounts.find(
        (a) =>
          a.homeAccountId === autoBackupTarget.homeAccountId &&
          a.cloudType === autoBackupTarget.cloudEnv
      ) ?? null
    : null;

  const toggleItem = (item: BackupItem) => {
    const next = autoBackupItems.includes(item)
      ? autoBackupItems.filter((i) => i !== item)
      : [...autoBackupItems, item];
    setAutoBackupItems(BACKUP_ITEMS.filter((i) => next.includes(i)));
  };

  const handleExport = async () => {
    const path = await save({ defaultPath: defaultBackupName() });
    if (!path) return;
    try {
      await exportConfig(path);
      addToast("success", t("backup.exportDone"));
    } catch (err) {
      addToast("error", getErrorMessage(err));
    }
  };

  const handleChooseImportFile = async () => {
    const path = await open({
      filters: [{ name: "ShareOneList backup", extensions: ["json"] }],
      multiple: false,
    });
    if (typeof path !== "string" || !path) return;
    try {
      const info = await inspectBackup(path);
      setImportPassword("");
      setPendingImport({ path, info, selected: info.items });
    } catch (err) {
      addToast("error", getErrorMessage(err));
    }
  };

  const toggleImportItem = (item: BackupItem) => {
    setPendingImport((prev) => {
      if (!prev) return prev;
      const selected = prev.selected.includes(item)
        ? prev.selected.filter((i) => i !== item)
        : [...prev.selected, item];
      return { ...prev, selected };
    });
  };

  const handleConfirmImport = async () => {
    if (!pendingImport) return;
    try {
      const summary: ImportSummary = await importConfig(
        pendingImport.path,
        pendingImport.info.encrypted ? importPassword : null,
        pendingImport.selected.length === pendingImport.info.items.length
          ? null
          : pendingImport.selected
      );
      // Reload settings and accounts from the freshly imported backend state.
      await useSettingsStore.getState().loadConfig();
      await useAuthStore.getState().loadAccounts();
      addToast(
        "success",
        t("backup.importDone", { count: summary.accountsCount })
      );
      setPendingImport(null);
    } catch (err) {
      addToast("error", getErrorMessage(err));
    }
  };

  const handleSavePassword = async () => {
    if (newPassword.length < 6) {
      addToast("error", t("backup.enc.tooShort"));
      return;
    }
    if (newPassword !== confirmPassword) {
      addToast("error", t("backup.enc.mismatch"));
      return;
    }
    setSavingPassword(true);
    try {
      await setBackupPassword(newPassword);
      setPasswordSet(true);
      setPasswordEditing(false);
      setNewPassword("");
      setConfirmPassword("");
      addToast("success", t("backup.enc.saved"));
    } catch (err) {
      addToast("error", getErrorMessage(err));
    } finally {
      setSavingPassword(false);
    }
  };

  const handleConfirmTarget = async (target: AutoBackupTarget) => {
    try {
      // Persists the target and uploads the first backup right away, so a
      // failure here means the destination is not usable.
      await setAutoBackupTarget(target);
      await loadConfig();
      addToast("success", t("backup.autoSetDone"));
      setPickerOpen(false);
    } catch (err) {
      addToast("error", getErrorMessage(err));
    }
  };

  const handleClearTarget = async () => {
    try {
      await setAutoBackupTarget(null);
      await loadConfig();
      addToast("success", t("backup.autoCleared"));
    } catch (err) {
      addToast("error", getErrorMessage(err));
    }
  };

  return (
    <>
      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h3 className="text-lg font-semibold text-foreground">{t("backup.title")}</h3>
        <p className="text-xs text-muted-foreground">{t("backup.noTokens")}</p>

        <div className="flex flex-wrap gap-2">
          <button
            onClick={handleExport}
            className="inline-flex items-center gap-2 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
          >
            <Download className="h-4 w-4" />
            {t("backup.export")}
          </button>
          <button
            onClick={handleChooseImportFile}
            className="inline-flex items-center gap-2 rounded-md border border-border bg-background px-3 py-1.5 text-sm font-medium text-foreground hover:bg-accent transition-colors"
          >
            <Upload className="h-4 w-4" />
            {t("backup.import")}
          </button>
        </div>

        {pendingImport && (
          <div className="space-y-2 rounded-lg border border-border bg-background p-3">
            <p className="break-all text-xs text-muted-foreground">
              {t("backup.importFile")}: {pendingImport.path}
            </p>
            {pendingImport.info.items.length > 0 && (
              <div className="space-y-1">
                <p className="text-sm font-medium text-foreground">
                  {t("backup.importChooseItems")}
                </p>
                {pendingImport.info.items.map((item) => (
                  <label
                    key={item}
                    className="flex cursor-pointer items-center gap-2 text-sm text-foreground"
                  >
                    <input
                      type="checkbox"
                      checked={pendingImport.selected.includes(item)}
                      onChange={() => toggleImportItem(item)}
                      className="h-3.5 w-3.5"
                    />
                    {t(`backup.items.${item}.name`)}
                  </label>
                ))}
              </div>
            )}
            {pendingImport.info.encrypted && (
              <div>
                <label className="text-sm font-medium text-foreground block mb-1">
                  {t("backup.enc.importPrompt")}
                </label>
                <input
                  type="password"
                  value={importPassword}
                  onChange={(e) => setImportPassword(e.target.value)}
                  className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-ring"
                  autoFocus
                />
              </div>
            )}
            <p className="text-sm text-foreground">{t("backup.importConfirm")}</p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setPendingImport(null)}
                className="rounded-md px-3 py-1.5 text-sm text-muted-foreground hover:bg-accent transition-colors"
              >
                {t("dialogs.cancel")}
              </button>
              <button
                onClick={handleConfirmImport}
                disabled={
                  pendingImport.info.encrypted && importPassword.length === 0
                }
                className="rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
              >
                {t("backup.importConfirmButton")}
              </button>
            </div>
          </div>
        )}
      </section>

      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h3 className="text-lg font-semibold text-foreground">{t("backup.itemsTitle")}</h3>
        <p className="text-xs text-muted-foreground">{t("backup.itemsHint")}</p>
        <div className="space-y-1.5">
          {BACKUP_ITEMS.map((item) => (
            <label
              key={item}
              className="flex cursor-pointer items-start gap-2.5 rounded-md px-2 py-1.5 text-sm hover:bg-accent/50 transition-colors"
            >
              <input
                type="checkbox"
                checked={autoBackupItems.includes(item)}
                onChange={() => toggleItem(item)}
                className="mt-0.5 h-3.5 w-3.5"
              />
              <span className="min-w-0">
                <span className="block font-medium text-foreground">
                  {t(`backup.items.${item}.name`)}
                </span>
                <span className="block text-xs text-muted-foreground">
                  {t(`backup.items.${item}.desc`)}
                </span>
              </span>
            </label>
          ))}
        </div>

        <div className="space-y-2 rounded-md border border-border bg-background p-3">
          <div className="flex items-center gap-2 text-sm">
            <ShieldCheck className="h-4 w-4 shrink-0 text-muted-foreground" />
            <span className="font-medium text-foreground">{t("backup.enc.title")}</span>
            <span
              className={
                passwordSet
                  ? "text-xs text-muted-foreground"
                  : "text-xs font-medium text-destructive"
              }
            >
              {passwordSet
                ? t("backup.enc.isSet")
                : t("backup.enc.notSet")}
            </span>
          </div>
          <p className="text-xs text-muted-foreground">{t("backup.enc.hint")}</p>
          {!passwordSet && autoBackupItems.length > 0 && (
            <p className="text-xs text-destructive">{t("backup.enc.needed")}</p>
          )}
          {(!passwordSet || passwordEditing) && (
            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <KeyRound className="h-4 w-4 shrink-0 text-muted-foreground" />
                <input
                  type="password"
                  value={newPassword}
                  onChange={(e) => setNewPassword(e.target.value)}
                  placeholder={t("backup.enc.newPassword")}
                  className="min-w-0 flex-1 rounded-md border border-border bg-background px-3 py-1.5 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring"
                />
                <input
                  type="password"
                  value={confirmPassword}
                  onChange={(e) => setConfirmPassword(e.target.value)}
                  placeholder={t("backup.enc.confirmPassword")}
                  className="min-w-0 flex-1 rounded-md border border-border bg-background px-3 py-1.5 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring"
                />
              </div>
              <div className="flex justify-end gap-2">
                {passwordEditing && (
                  <button
                    onClick={() => {
                      setPasswordEditing(false);
                      setNewPassword("");
                      setConfirmPassword("");
                    }}
                    className="rounded-md px-3 py-1.5 text-sm text-muted-foreground hover:bg-accent transition-colors"
                  >
                    {t("dialogs.cancel")}
                  </button>
                )}
                <button
                  onClick={handleSavePassword}
                  disabled={savingPassword || newPassword.length < 6}
                  className="rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
                >
                  {t("backup.enc.setPassword")}
                </button>
              </div>
            </div>
          )}
          {passwordSet && !passwordEditing && (
            <div className="flex justify-end">
              <button
                onClick={() => setPasswordEditing(true)}
                className="rounded-md px-3 py-1.5 text-sm text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
              >
                {t("backup.enc.change")}
              </button>
            </div>
          )}
        </div>
      </section>

      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h3 className="text-lg font-semibold text-foreground">{t("backup.autoTitle")}</h3>
        <p className="text-xs text-muted-foreground">{t("backup.autoHint")}</p>
        <div className="flex items-center gap-2">
          <div className="flex min-w-0 flex-1 items-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm">
            {autoBackupTarget ? (
              <>
                <Cloud className="h-4 w-4 shrink-0 text-muted-foreground" />
                <span className="shrink-0 font-medium text-foreground">
                  {targetAccount
                    ? accountDisplayName(targetAccount)
                    : t("backup.autoAccountMissing")}
                </span>
                <span className="truncate text-muted-foreground" title={autoBackupTarget.folderPath}>
                  {autoBackupTarget.folderPath}
                </span>
              </>
            ) : (
              <span className="text-foreground">{t("backup.autoOff")}</span>
            )}
          </div>
          <button
            onClick={() => setPickerOpen(true)}
            className="inline-flex shrink-0 items-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm font-medium text-foreground hover:bg-accent transition-colors"
          >
            <FolderOpen className="h-4 w-4" />
            {t("backup.autoChoose")}
          </button>
          {autoBackupTarget && (
            <button
              onClick={handleClearTarget}
              className="shrink-0 rounded-md px-3 py-2 text-sm text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
            >
              {t("backup.autoClear")}
            </button>
          )}
        </div>
      </section>

      {pickerOpen && (
        <CloudBackupFolderPickerDialog
          initial={autoBackupTarget}
          onClose={() => setPickerOpen(false)}
          onConfirm={handleConfirmTarget}
        />
      )}
    </>
  );
}
