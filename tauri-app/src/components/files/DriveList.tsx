import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowLeft, Database, RefreshCw, AlertCircle } from "lucide-react";
import { getSiteDrives, getSharedDrives } from "../../lib/tauri";
import { getErrorMessage } from "../../lib/errors";
import type { CloudEnvironment, Drive } from "../../lib/types";

interface DriveListProps {
  mode: "sharepoint" | "shared";
  siteId?: string;
  siteName?: string;
  cloudEnv: CloudEnvironment;
  homeAccountId: string;
  onDriveSelect: (driveId: string, driveName: string, cloudEnv: CloudEnvironment) => void;
  onBack: () => void;
}

/** Format bytes to human-readable size. */
function formatSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const value = bytes / Math.pow(1024, i);
  return `${value.toFixed(1)} ${units[i]}`;
}

/**
 * Lists document libraries (drives) for a SharePoint site or shared drives.
 * Handles loading, empty, and error states with retry support.
 */
export function DriveList({
  mode,
  siteId,
  siteName,
  cloudEnv,
  homeAccountId,
  onDriveSelect,
  onBack,
}: DriveListProps) {
  const { t } = useTranslation();
  const [drives, setDrives] = useState<Drive[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchDrives = async () => {
    setIsLoading(true);
    setError(null);
    try {
      if (mode === "sharepoint" && siteId) {
        const result = await getSiteDrives(siteId, cloudEnv, homeAccountId);
        setDrives(result);
      } else if (mode === "shared") {
        const result = await getSharedDrives(cloudEnv, homeAccountId);
        setDrives(result);
      }
    } catch (err) {
      setError(getErrorMessage(err));
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    fetchDrives();
  }, [mode, siteId, homeAccountId]);

  const title =
    mode === "sharepoint" && siteName
      ? siteName
      : t("driveHub.sharedWithMe");

  const emptyMessage =
    mode === "sharepoint"
      ? t("driveHub.noSharePoint")
      : t("driveHub.noSharedDrives");

  const emptyDescription =
    mode === "sharepoint"
      ? t("driveHub.noSharePointDesc")
      : t("driveHub.noSharedDrivesDesc");

  return (
    <div className="space-y-4">
      {/* Header with back button */}
      <div className="flex items-center gap-3">
        <button
          onClick={onBack}
          className="rounded-md p-1.5 text-muted-foreground hover:bg-accent transition-colors"
          aria-label={t("files.back")}
        >
          <ArrowLeft className="h-5 w-5" />
        </button>
        <h3 className="text-lg font-semibold text-foreground">
          {title}
        </h3>
      </div>

      {/* Loading state */}
      {isLoading && (
        <div className="flex items-center justify-center py-12">
          <RefreshCw className="h-5 w-5 animate-spin text-muted-foreground" />
        </div>
      )}

      {/* Error state */}
      {!isLoading && error && (
        <div className="rounded-lg border border-destructive/50 bg-destructive/10 p-6 text-center space-y-3">
          <AlertCircle className="h-8 w-8 text-destructive mx-auto" />
          <p className="text-sm text-destructive">{error}</p>
          <button
            onClick={fetchDrives}
            className="inline-flex items-center gap-2 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
          >
            <RefreshCw className="h-4 w-4" />
            {t("errors.retryAction")}
          </button>
        </div>
      )}

      {/* Empty state */}
      {!isLoading && !error && drives.length === 0 && (
        <div className="rounded-lg border border-dashed border-border p-8 text-center space-y-2">
          <Database className="h-8 w-8 text-muted-foreground mx-auto" />
          <p className="text-sm font-medium text-foreground">
            {emptyMessage}
          </p>
          <p className="text-xs text-muted-foreground">
            {emptyDescription}
          </p>
        </div>
      )}

      {/* Drives list */}
      {!isLoading && !error && drives.length > 0 && (
        <div className="space-y-2">
          {drives.map((drive) => (
            <button
              key={drive.id}
              onClick={() => onDriveSelect(drive.id, drive.name, cloudEnv)}
              className="w-full flex items-center gap-3 rounded-lg border border-border bg-card px-4 py-3 hover:bg-accent/30 transition-colors cursor-pointer text-left"
            >
              <Database className="h-5 w-5 text-blue-500 shrink-0" />
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium text-foreground truncate">
                  {drive.name}
                </div>
                {drive.quota && (
                  <div className="text-xs text-muted-foreground">
                    {t("storage.used")}: {formatSize(drive.quota.used)} / {formatSize(drive.quota.total)}
                  </div>
                )}
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
