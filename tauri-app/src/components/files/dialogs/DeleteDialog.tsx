import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import type { CloudEnvironment, DriveItem } from "../../../lib/types";
import { deleteItem } from "../../../lib/tauri";
import { useToastStore } from "../../../stores/toastStore";

interface DeleteDialogProps {
  item: DriveItem;
  cloudEnv: CloudEnvironment;
  driveId: string;
  onClose: () => void;
  onSuccess: () => void;
}

/**
 * Confirmation dialog for deleting a file or folder.
 * The item is soft-deleted (moved to the recycle bin).
 */
export function DeleteDialog({ item, cloudEnv, driveId, onClose, onSuccess }: DeleteDialogProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !isLoading) onClose();
    };
    document.addEventListener("keydown", handleEscape);
    return () => document.removeEventListener("keydown", handleEscape);
  }, [isLoading, onClose]);

  const handleDelete = async () => {
    setIsLoading(true);
    setError(null);
    try {
      await deleteItem(driveId, item.id, cloudEnv);
      onSuccess();
      onClose();
    } catch (err) {
      const message = err instanceof Error ? err.message : t("errors.deleteFailed");
      setError(message);
      addToast("error", message);
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-labelledby="delete-dialog-title"
    >
      <div
        className="w-full max-w-md rounded-lg bg-card p-6 shadow-lg border border-border"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="delete-dialog-title" className="text-lg font-semibold text-foreground mb-4">
          {t("dialogs.delete.title")}
        </h3>

        <p className="text-sm text-foreground mb-2">
          {t("dialogs.delete.message", { name: item.name })}
        </p>

        {error && (
          <div className="mb-4 rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}

        <div className="flex justify-end gap-2 mt-6">
          <button
            onClick={onClose}
            disabled={isLoading}
            className="rounded-md px-4 py-2 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors disabled:opacity-50"
          >
            {t("dialogs.cancel")}
          </button>
          <button
            onClick={handleDelete}
            disabled={isLoading}
            className="inline-flex items-center gap-2 rounded-md bg-destructive px-4 py-2 text-sm font-medium text-destructive-foreground hover:bg-destructive/90 transition-colors disabled:opacity-50"
          >
            {isLoading && <Loader2 className="h-4 w-4 animate-spin" />}
            {t("fileOps.delete")}
          </button>
        </div>
      </div>
    </div>
  );
}
