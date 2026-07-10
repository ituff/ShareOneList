import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getDriveQuota } from "../../lib/tauri";
import { formatFileSize } from "../../lib/formatters";
import type { DriveQuota, CloudEnvironment } from "../../lib/types";

interface StorageInfoProps {
  driveId: string;
  cloudEnv: CloudEnvironment;
}

type LoadingState = "loading" | "loaded" | "error";

/**
 * Displays storage quota information for a drive with a color-coded progress bar.
 * Color coding: green (<70%), yellow (70-90%), red (>90%).
 */
export function StorageInfo({ driveId, cloudEnv }: StorageInfoProps) {
  const { t } = useTranslation();
  const [quota, setQuota] = useState<DriveQuota | null>(null);
  const [state, setState] = useState<LoadingState>("loading");

  useEffect(() => {
    let cancelled = false;

    async function fetchQuota() {
      setState("loading");
      try {
        const data = await getDriveQuota(driveId);
        if (!cancelled) {
          setQuota(data);
          setState("loaded");
        }
      } catch {
        if (!cancelled) {
          setState("error");
        }
      }
    }

    fetchQuota();
    return () => {
      cancelled = true;
    };
  }, [driveId, cloudEnv]);

  // Loading skeleton
  if (state === "loading") {
    return (
      <div className="rounded-lg border border-border bg-card p-4 animate-pulse">
        <div className="h-4 w-24 bg-muted rounded mb-3" />
        <div className="h-2.5 w-full bg-muted rounded-full mb-3" />
        <div className="flex gap-4">
          <div className="h-3 w-20 bg-muted rounded" />
          <div className="h-3 w-20 bg-muted rounded" />
          <div className="h-3 w-20 bg-muted rounded" />
        </div>
      </div>
    );
  }

  // Error state
  if (state === "error") {
    return (
      <div className="rounded-lg border border-destructive/50 bg-destructive/5 p-4">
        <p className="text-sm text-destructive">
          {t("storage.unavailable")}
        </p>
      </div>
    );
  }

  // Loaded state
  if (!quota) return null;

  const usedRatio = quota.total > 0 ? (quota.used / quota.total) * 100 : 0;

  // Color coding based on usage percentage
  const getBarColor = (percent: number): string => {
    if (percent > 90) return "bg-red-500";
    if (percent > 70) return "bg-yellow-500";
    return "bg-green-500";
  };

  const getBarTrackColor = (percent: number): string => {
    if (percent > 90) return "bg-red-100 dark:bg-red-950";
    if (percent > 70) return "bg-yellow-100 dark:bg-yellow-950";
    return "bg-green-100 dark:bg-green-950";
  };

  return (
    <div className="rounded-lg border border-border bg-card p-4">
      {/* Progress bar */}
      <div
        className={`h-2.5 w-full rounded-full overflow-hidden mb-3 ${getBarTrackColor(usedRatio)}`}
        role="progressbar"
        aria-valuenow={Math.round(usedRatio)}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label={`${Math.round(usedRatio)}% storage used`}
      >
        <div
          className={`h-full rounded-full transition-all duration-300 ${getBarColor(usedRatio)}`}
          style={{ width: `${Math.min(usedRatio, 100)}%` }}
        />
      </div>

      {/* Storage details */}
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
        <span>
          {t("storage.used")}: <span className="font-medium text-foreground">{formatFileSize(quota.used)}</span>
        </span>
        <span>
          {t("storage.remaining")}: <span className="font-medium text-foreground">{formatFileSize(quota.remaining)}</span>
        </span>
        <span>
          {t("storage.total")}: <span className="font-medium text-foreground">{formatFileSize(quota.total)}</span>
        </span>
      </div>
    </div>
  );
}
