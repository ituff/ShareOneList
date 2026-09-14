import { useState } from "react";
import { useTranslation } from "react-i18next";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Download, FolderOpen, Upload } from "lucide-react";
import { exportConfig, importConfig } from "../../lib/tauri";
import type { ImportSummary } from "../../lib/tauri";
import { useSettingsStore } from "../../stores/settingsStore";
import { useAuthStore } from "../../stores/authStore";
import { useToastStore } from "../../stores/toastStore";

function defaultBackupName(): string {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `ShareOneList-backup-${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(
    now.getDate()
  )}-${pad(now.getHours())}${pad(now.getMinutes())}.json`;
}

/** The "Backup & restore" tab: manual export/import of a backup file and
 * automatic backups into a user-chosen OneDrive sync folder. */
export function BackupSettings() {
  const { t } = useTranslation();
  const autoBackupDir = useSettingsStore((s) => s.autoBackupDir);
  const setAutoBackupDir = useSettingsStore((s) => s.setAutoBackupDir);
  const addToast = useToastStore((s) => s.addToast);
  const [pendingImport, setPendingImport] = useState<string | null>(null);

  const handleExport = async () => {
    const path = await save({ defaultPath: defaultBackupName() });
    if (!path) return;
    try {
      await exportConfig(path);
      addToast("success", t("backup.exportDone"));
    } catch (err) {
      addToast("error", String(err));
    }
  };

  const handleChooseImportFile = async () => {
    const path = await open({
      filters: [{ name: "ShareOneList backup", extensions: ["json"] }],
      multiple: false,
    });
    if (typeof path === "string" && path) setPendingImport(path);
  };

  const handleConfirmImport = async () => {
    if (!pendingImport) return;
    try {
      const summary: ImportSummary = await importConfig(pendingImport);
      // Reload settings and accounts from the freshly imported backend state.
      await useSettingsStore.getState().loadConfig();
      await useAuthStore.getState().loadAccounts();
      addToast(
        "success",
        t("backup.importDone", { count: summary.accountsCount })
      );
      setPendingImport(null);
    } catch (err) {
      addToast("error", String(err));
      setPendingImport(null);
    }
  };

  const handleChooseAutoDir = async () => {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== "string" || !dir) return;
    try {
      await setAutoBackupDir(dir);
      addToast("success", t("backup.autoSetDone"));
    } catch (err) {
      addToast("error", String(err));
    }
  };

  const handleClearAutoDir = async () => {
    try {
      await setAutoBackupDir(null);
      addToast("success", t("backup.autoCleared"));
    } catch (err) {
      addToast("error", String(err));
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
              {t("backup.importFile")}: {pendingImport}
            </p>
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
                className="rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
              >
                {t("backup.importConfirmButton")}
              </button>
            </div>
          </div>
        )}
      </section>

      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h3 className="text-lg font-semibold text-foreground">{t("backup.autoTitle")}</h3>
        <p className="text-xs text-muted-foreground">{t("backup.autoHint")}</p>
        <div className="flex items-center gap-2">
          <div className="min-w-0 flex-1 truncate rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground">
            {autoBackupDir || t("backup.autoOff")}
          </div>
          <button
            onClick={handleChooseAutoDir}
            className="inline-flex shrink-0 items-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm font-medium text-foreground hover:bg-accent transition-colors"
          >
            <FolderOpen className="h-4 w-4" />
            {t("backup.autoChoose")}
          </button>
          {autoBackupDir && (
            <button
              onClick={handleClearAutoDir}
              className="shrink-0 rounded-md px-3 py-2 text-sm text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
            >
              {t("backup.autoClear")}
            </button>
          )}
        </div>
      </section>
    </>
  );
}
