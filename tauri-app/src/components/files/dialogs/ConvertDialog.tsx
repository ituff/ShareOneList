import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, FileDown } from "lucide-react";
import type { CloudEnvironment, DriveItem } from "../../../lib/types";
import { convertFormat } from "../../../lib/tauri";
import { useToastStore } from "../../../stores/toastStore";

/** File extensions that support PDF conversion. */
const CONVERTIBLE_EXTENSIONS = [".docx", ".doc", ".xlsx", ".xls", ".pptx", ".ppt"];

interface ConvertDialogProps {
  item: DriveItem;
  driveId: string;
  cloudEnv: CloudEnvironment;
  onClose: () => void;
}

/**
 * Check if a file can be converted to PDF based on its extension.
 */
export function canConvertToPdf(fileName: string): boolean {
  const lower = fileName.toLowerCase();
  return CONVERTIBLE_EXTENSIONS.some((ext) => lower.endsWith(ext));
}

/**
 * Modal dialog for converting a Word/Excel/PowerPoint document to PDF.
 * Prompts the user for a save filename, then invokes the backend conversion command.
 */
export function ConvertDialog({ item, driveId, cloudEnv: _cloudEnv, onClose }: ConvertDialogProps) {
  const { t } = useTranslation();
  const addToast = useToastStore((s) => s.addToast);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Default output filename: replace extension with .pdf
  const defaultName = item.name.replace(/\.[^.]+$/, "") + ".pdf";
  const [saveName, setSaveName] = useState(defaultName);

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !isLoading) onClose();
    };
    document.addEventListener("keydown", handleEscape);
    return () => document.removeEventListener("keydown", handleEscape);
  }, [isLoading, onClose]);

  const handleConvert = async () => {
    const trimmed = saveName.trim();
    if (!trimmed) return;

    setIsLoading(true);
    setError(null);

    try {
      // Use the filename as the save path — the backend will handle the actual save location
      const savePath = trimmed.endsWith(".pdf") ? trimmed : trimmed + ".pdf";

      // Try to use Tauri dialog plugin if available
      let finalPath = savePath;
      try {
        // Dynamic import bypasses TS module resolution check at compile time
        const modulePath = "@tauri-apps/plugin-dialog";
        const dialogModule = await import(/* @vite-ignore */ modulePath);
        const selected = await dialogModule.save({
          defaultPath: savePath,
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        });
        if (!selected) {
          setIsLoading(false);
          return; // user cancelled
        }
        finalPath = selected as string;
      } catch {
        // Dialog plugin not available — use filename directly as path
        // The backend will save to a default downloads location
      }

      await convertFormat(driveId, item.id, "pdf", finalPath);
      addToast("success", t("dialogs.convert.success"));
      onClose();
    } catch (err) {
      const message = err instanceof Error ? err.message : t("errors.unknownError");
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
      aria-labelledby="convert-dialog-title"
    >
      <div
        className="w-full max-w-md rounded-lg bg-card p-6 shadow-lg border border-border"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="convert-dialog-title" className="text-lg font-semibold text-foreground mb-4 flex items-center gap-2">
          <FileDown className="h-5 w-5 text-primary" />
          {t("dialogs.convert.title")}
        </h3>

        <p className="text-sm text-muted-foreground mb-4">
          {t("dialogs.convert.message", { name: item.name })}
        </p>

        {/* Save filename input */}
        <div className="mb-4">
          <label className="text-sm font-medium text-foreground block mb-1">
            {t("dialogs.convert.saveAs")}
          </label>
          <input
            type="text"
            value={saveName}
            onChange={(e) => setSaveName(e.target.value)}
            className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-ring"
            disabled={isLoading}
            aria-label={t("dialogs.convert.saveAs")}
            autoFocus
          />
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
            onClick={handleConvert}
            disabled={isLoading || !saveName.trim()}
            className="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50"
          >
            {isLoading && <Loader2 className="h-4 w-4 animate-spin" />}
            {t("dialogs.convert.convertBtn")}
          </button>
        </div>
      </div>
    </div>
  );
}
