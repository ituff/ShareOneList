import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  X,
  ChevronLeft,
  ChevronRight,
  Loader2,
  AlertCircle,
  Download,
  RotateCw,
} from "lucide-react";
import { getPreviewUrl } from "../../lib/tauri";
import type { CloudEnvironment, DriveItem } from "../../lib/types";

// ─── Helpers ────────────────────────────────────────────────────────────────

const IMAGE_EXTENSIONS = [".png", ".jpg", ".jpeg", ".gif", ".bmp", ".svg", ".webp"];
const DOC_EXTENSIONS = [".docx", ".doc", ".xlsx", ".xls", ".pptx", ".ppt", ".pdf"];

/** Check if a DriveItem is previewable (image or document). */
export function isPreviewable(item: DriveItem): boolean {
  if (item.isFolder) return false;
  const name = item.name.toLowerCase();
  return (
    IMAGE_EXTENSIONS.some((ext) => name.endsWith(ext)) ||
    DOC_EXTENSIONS.some((ext) => name.endsWith(ext))
  );
}

/** Check if a DriveItem is an image file. */
export function isImageFile(item: DriveItem): boolean {
  const name = item.name.toLowerCase();
  return IMAGE_EXTENSIONS.some((ext) => name.endsWith(ext));
}

// ─── Props ──────────────────────────────────────────────────────────────────

interface FilePreviewProps {
  item: DriveItem;
  driveId: string;
  cloudEnv: CloudEnvironment;
  /** All previewable items in the current folder (for next/prev navigation) */
  previewableItems: DriveItem[];
  /** Index of the current item in previewableItems */
  currentIndex: number;
  onClose: () => void;
  onNavigate: (index: number) => void;
}

// ─── Component ──────────────────────────────────────────────────────────────

export function FilePreview({
  item,
  driveId,
  cloudEnv: _cloudEnv,
  previewableItems,
  currentIndex,
  onClose,
  onNavigate,
}: FilePreviewProps) {
  // _cloudEnv reserved for future environment-specific preview logic
  void _cloudEnv;
  const { t } = useTranslation();
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isImage = isImageFile(item);
  const hasPrev = currentIndex > 0;
  const hasNext = currentIndex < previewableItems.length - 1;

  // Load preview URL
  const loadPreview = useCallback(async () => {
    setError(null);

    if (isImage) {
      // For images, use downloadUrl directly if available
      if (item.downloadUrl) {
        setPreviewUrl(item.downloadUrl);
        return;
      }
    }

    // For documents or images without downloadUrl, fetch preview URL from backend
    setIsLoading(true);
    try {
      const url = await getPreviewUrl(driveId, item.id);
      setPreviewUrl(url);
    } catch (err) {
      const message = err instanceof Error ? err.message : t("errors.unknownError");
      setError(message);
    } finally {
      setIsLoading(false);
    }
  }, [item, driveId, isImage, t]);

  useEffect(() => {
    setPreviewUrl(null);
    loadPreview();
  }, [loadPreview]);

  // Keyboard navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      switch (e.key) {
        case "Escape":
          onClose();
          break;
        case "ArrowLeft":
          if (hasPrev) onNavigate(currentIndex - 1);
          break;
        case "ArrowRight":
          if (hasNext) onNavigate(currentIndex + 1);
          break;
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onClose, onNavigate, currentIndex, hasPrev, hasNext]);

  // Prevent body scroll when modal is open
  useEffect(() => {
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = "";
    };
  }, []);

  const handleRetry = () => {
    loadPreview();
  };

  const handleDownload = () => {
    if (item.downloadUrl) {
      window.open(item.downloadUrl, "_blank");
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/80"
      role="dialog"
      aria-modal="true"
      aria-label={t("preview.title")}
    >
      {/* Close button */}
      <button
        onClick={onClose}
        className="absolute top-4 right-4 z-10 rounded-full bg-black/50 p-2 text-white hover:bg-black/70 transition-colors"
        aria-label={t("dialogs.close")}
      >
        <X className="h-5 w-5" />
      </button>

      {/* File name */}
      <div className="absolute top-4 left-4 z-10 max-w-[60%] truncate rounded bg-black/50 px-3 py-1.5 text-sm text-white">
        {item.name}
      </div>

      {/* Previous button */}
      {hasPrev && (
        <button
          onClick={() => onNavigate(currentIndex - 1)}
          className="absolute left-4 top-1/2 -translate-y-1/2 z-10 rounded-full bg-black/50 p-2 text-white hover:bg-black/70 transition-colors"
          aria-label={t("preview.previous")}
        >
          <ChevronLeft className="h-6 w-6" />
        </button>
      )}

      {/* Next button */}
      {hasNext && (
        <button
          onClick={() => onNavigate(currentIndex + 1)}
          className="absolute right-4 top-1/2 -translate-y-1/2 z-10 rounded-full bg-black/50 p-2 text-white hover:bg-black/70 transition-colors"
          aria-label={t("preview.next")}
        >
          <ChevronRight className="h-6 w-6" />
        </button>
      )}

      {/* Preview content */}
      <div className="flex h-full w-full items-center justify-center p-16">
        {/* Loading */}
        {isLoading && (
          <div className="flex flex-col items-center gap-3">
            <Loader2 className="h-8 w-8 animate-spin text-white" />
            <p className="text-sm text-white/70">{t("preview.loading")}</p>
          </div>
        )}

        {/* Error */}
        {!isLoading && error && (
          <div className="flex flex-col items-center gap-4 rounded-lg bg-card p-6 shadow-lg">
            <AlertCircle className="h-10 w-10 text-destructive" />
            <p className="text-sm text-muted-foreground">{t("preview.loadFailed")}</p>
            <div className="flex items-center gap-3">
              <button
                onClick={handleRetry}
                className="flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
              >
                <RotateCw className="h-3.5 w-3.5" />
                {t("preview.retry")}
              </button>
              {item.downloadUrl && (
                <button
                  onClick={handleDownload}
                  className="flex items-center gap-1.5 rounded-md border border-border px-3 py-1.5 text-sm font-medium text-foreground hover:bg-accent transition-colors"
                >
                  <Download className="h-3.5 w-3.5" />
                  {t("preview.downloadInstead")}
                </button>
              )}
            </div>
          </div>
        )}

        {/* Image preview */}
        {!isLoading && !error && previewUrl && isImage && (
          <img
            src={previewUrl}
            alt={item.name}
            className="max-h-full max-w-full object-contain rounded"
            onError={() => setError(t("preview.loadFailed"))}
          />
        )}

        {/* Document preview (iframe) */}
        {!isLoading && !error && previewUrl && !isImage && (
          <iframe
            src={previewUrl}
            title={item.name}
            className="h-full w-full rounded bg-white"
            sandbox="allow-scripts allow-same-origin allow-popups"
          />
        )}
      </div>

      {/* Counter indicator */}
      {previewableItems.length > 1 && (
        <div className="absolute bottom-4 left-1/2 -translate-x-1/2 z-10 rounded bg-black/50 px-3 py-1 text-sm text-white">
          {currentIndex + 1} / {previewableItems.length}
        </div>
      )}
    </div>
  );
}
