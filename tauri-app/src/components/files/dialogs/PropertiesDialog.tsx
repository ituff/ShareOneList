import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, ExternalLink, FileText, Folder } from "lucide-react";
import type { CloudEnvironment, DriveItem, ItemProperties } from "../../../lib/types";
import { getItemProperties } from "../../../lib/tauri";
import { formatFileSize, formatDate } from "../../../lib/formatters";

interface PropertiesDialogProps {
  item: DriveItem;
  driveId: string;
  cloudEnv: CloudEnvironment;
  onClose: () => void;
}

/**
 * Modal dialog displaying file/folder metadata properties.
 * Fetches fresh data from the backend on open.
 */
export function PropertiesDialog({ item, driveId, cloudEnv: _cloudEnv, onClose }: PropertiesDialogProps) {
  const { t } = useTranslation();
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [properties, setProperties] = useState<ItemProperties | null>(null);

  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handleEscape);
    return () => document.removeEventListener("keydown", handleEscape);
  }, [onClose]);

  useEffect(() => {
    let cancelled = false;

    async function fetchProperties() {
      setIsLoading(true);
      setError(null);
      try {
        const props = await getItemProperties(driveId, item.id);
        if (!cancelled) {
          setProperties(props);
        }
      } catch (err) {
        if (!cancelled) {
          const message = err instanceof Error ? err.message : t("errors.unknownError");
          setError(message);
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    }

    fetchProperties();
    return () => { cancelled = true; };
  }, [driveId, item.id, t]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-labelledby="properties-dialog-title"
    >
      <div
        className="w-full max-w-md rounded-lg bg-card p-6 shadow-lg border border-border"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="properties-dialog-title" className="text-lg font-semibold text-foreground mb-4 flex items-center gap-2">
          {item.isFolder ? (
            <Folder className="h-5 w-5 text-primary" />
          ) : (
            <FileText className="h-5 w-5 text-primary" />
          )}
          {t("dialogs.properties.title")}
        </h3>

        {isLoading ? (
          <div className="flex items-center justify-center py-8">
            <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
          </div>
        ) : error ? (
          <div className="mb-4 rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        ) : properties ? (
          <div className="space-y-3">
            {/* Name */}
            <PropertyRow label={t("properties.filename")} value={properties.name} />

            {/* Size */}
            {properties.size != null && (
              <PropertyRow
                label={t("properties.size")}
                value={formatFileSize(properties.size)}
              />
            )}

            {/* Created */}
            {properties.createdDateTime && (
              <PropertyRow
                label={t("properties.created")}
                value={formatDate(properties.createdDateTime)}
              />
            )}

            {/* Modified */}
            {properties.lastModifiedDateTime && (
              <PropertyRow
                label={t("properties.modified")}
                value={formatDate(properties.lastModifiedDateTime)}
              />
            )}

            {/* Web URL */}
            {properties.webUrl && (
              <div className="flex flex-col gap-1">
                <span className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                  {t("properties.webUrl")}
                </span>
                <a
                  href={properties.webUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center gap-1 text-sm text-primary hover:underline break-all"
                >
                  <ExternalLink className="h-3.5 w-3.5 flex-shrink-0" />
                  <span className="truncate">{properties.webUrl}</span>
                </a>
              </div>
            )}
          </div>
        ) : null}

        <div className="flex justify-end mt-6">
          <button
            onClick={onClose}
            className="rounded-md px-4 py-2 text-sm font-medium text-muted-foreground hover:bg-accent transition-colors"
          >
            {t("dialogs.close")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** A single property row with label and value. */
function PropertyRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
        {label}
      </span>
      <span className="text-sm text-foreground break-all">{value}</span>
    </div>
  );
}
