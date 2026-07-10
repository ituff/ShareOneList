import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import type { CloudEnvironment, DriveItem } from "../../../lib/types";
import { createFolder } from "../../../lib/tauri";
import { validateFileName } from "../../../lib/validators";
import { useToastStore } from "../../../stores/toastStore";

interface CreateFolderDialogProps {
  driveId: string;
  parentId: string;
  cloudEnv: CloudEnvironment;
  onClose: () => void;
  onSuccess: (newItem: DriveItem) => void;
}

/**
 * Modal dialog for creating a new folder.
 * Validates the folder name (1–400 chars, no invalid characters).
 */
export function CreateFolderDialog({
  driveId,
  parentId,
  cloudEnv,
  onClose,
  onSuccess,
}: CreateFolderDialogProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const [name, setName] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const validation = name.length > 0 ? validateFileName(name) : { valid: false, error: undefined };

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !isLoading) onClose();
    };
    document.addEventListener("keydown", handleEscape);
    return () => document.removeEventListener("keydown", handleEscape);
  }, [isLoading, onClose]);

  const handleSubmit = async () => {
    const result = validateFileName(name);
    if (!result.valid) return;

    setIsLoading(true);
    setError(null);
    try {
      const newItem = await createFolder(driveId, parentId, name, cloudEnv);
      onSuccess(newItem);
      onClose();
    } catch (err) {
      const message = err instanceof Error ? err.message : t("errors.createFolderFailed");
      setError(message);
      addToast("error", message);
    } finally {
      setIsLoading(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && validation.valid && !isLoading) {
      handleSubmit();
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-labelledby="create-folder-dialog-title"
    >
      <div
        className="w-full max-w-md rounded-lg bg-card p-6 shadow-lg border border-border"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="create-folder-dialog-title" className="text-lg font-semibold text-foreground mb-4">
          {t("dialogs.createFolder.title")}
        </h3>

        <div className="mb-4">
          <label className="text-sm font-medium text-foreground block mb-1">
            {t("dialogs.createFolder.label")}
          </label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={t("dialogs.createFolder.placeholder")}
            className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring"
            disabled={isLoading}
            autoFocus
            aria-label={t("dialogs.createFolder.label")}
          />
          {!validation.valid && name.length > 0 && validation.error && (
            <p className="mt-1 text-xs text-destructive">{validation.error}</p>
          )}
        </div>

        {error && (
          <div className="mb-4 rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}

        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            disabled={isLoading}
            className="rounded-md px-4 py-2 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors disabled:opacity-50"
          >
            {t("dialogs.cancel")}
          </button>
          <button
            onClick={handleSubmit}
            disabled={isLoading || !validation.valid}
            className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
          >
            {isLoading && <Loader2 className="h-4 w-4 animate-spin" />}
            {t("dialogs.create")}
          </button>
        </div>
      </div>
    </div>
  );
}
