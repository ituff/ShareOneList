import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Pause, Play, X, FolderOpen, Trash2, ShieldCheck, RefreshCw } from "lucide-react";
import { redownloadFiles, verifyBatchFiles } from "../../lib/tauri";
import { useTaskStore, type TaskEntry } from "../../stores/taskStore";
import { useToastStore } from "../../stores/toastStore";
import { useAuthStore } from "../../stores/authStore";
import { formatFileSize } from "../../lib/formatters";
import { getErrorMessage, isAuthError } from "../../lib/errors";
import {
  pauseDownload,
  resumeDownload,
  cancelDownload,
  removeDownloadTask,
  cancelUpload,
  openContainingFolder,
} from "../../lib/tauri";
import type { TaskStatus } from "../../lib/types";

interface TaskItemProps {
  task: TaskEntry;
}

/** Format seconds to mm:ss or hh:mm:ss. */
function formatElapsedTime(secs: number): string {
  const totalSeconds = Math.floor(secs);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  const mm = minutes.toString().padStart(2, "0");
  const ss = seconds.toString().padStart(2, "0");

  if (hours > 0) {
    const hh = hours.toString().padStart(2, "0");
    return `${hh}:${mm}:${ss}`;
  }
  return `${mm}:${ss}`;
}

/** Format transfer speed to human-readable string. */
function formatSpeed(bps: number): string {
  if (bps === 0) return "0 B/s";
  return `${formatFileSize(bps)}/s`;
}

/** Get progress percentage (0–100). */
function getProgress(task: TaskEntry): number {
  if (task.totalBytes === 0) return 0;
  return Math.min(100, Math.round((task.transferredBytes / task.totalBytes) * 100));
}

/** Status badge color classes. */
function getStatusBadgeClasses(status: TaskStatus): string {
  switch (status) {
    case "downloading":
    case "uploading":
      return "bg-blue-100 text-blue-700 dark:bg-blue-900 dark:text-blue-300";
    case "paused":
    case "queued":
      return "bg-yellow-100 text-yellow-700 dark:bg-yellow-900 dark:text-yellow-300";
    case "completed":
      return "bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300";
    case "failed":
      return "bg-red-100 text-red-700 dark:bg-red-900 dark:text-red-300";
    default:
      return "bg-muted text-muted-foreground";
  }
}

/** Human-readable status label. */
function getStatusLabel(status: TaskStatus): string {
  switch (status) {
    case "downloading":
      return "Downloading";
    case "uploading":
      return "Uploading";
    case "paused":
      return "Paused";
    case "queued":
      return "Queued";
    case "completed":
      return "Completed";
    case "failed":
      return "Failed";
    default:
      return status;
  }
}

export function TaskItem({ task }: TaskItemProps) {
  const { t } = useTranslation();
  const removeTask = useTaskStore((s) => s.removeTask);
  const addToast = useToastStore((s) => s.addToast);
  const [verifying, setVerifying] = useState(false);

  const progress = getProgress(task);
  const isActive = task.status === "downloading" || task.status === "uploading";

  const handlePause = async () => {
    try {
      await pauseDownload(task.taskId);
    } catch (err) {
      addToast("error", getErrorMessage(err));
    }
  };

  const handleResume = async () => {
    if (!task.cloudEnv) {
      addToast("error", t("errors.unknownError"));
      return;
    }
    try {
      await resumeDownload(task.cloudEnv, task.taskId);
    } catch (err) {
      if (isAuthError(err)) {
        useAuthStore.getState().setPendingRelogin({
          cloudEnv: task.cloudEnv,
          taskId: task.taskId,
        });
        return;
      }
      addToast("error", getErrorMessage(err));
    }
  };

  const handleCancelDownload = async () => {
    try {
      await cancelDownload(task.taskId);
    } catch {
      // Task may already be gone on backend
    }
    removeTask(task.taskId);
  };

  const handleCancelUpload = async () => {
    try {
      await cancelUpload(task.taskId);
    } catch {
      // Task may already be gone on backend
    }
    removeTask(task.taskId);
  };

  const handleOpenFolder = async () => {
    if (!task.localPath) {
      addToast("error", t("errors.unknownError"));
      return;
    }
    try {
      await openContainingFolder(task.localPath);
    } catch (err) {
      addToast("error", getErrorMessage(err));
    }
  };

  const handleRemove = async () => {
    try {
      await removeDownloadTask(task.taskId);
    } catch {
      // Task may already be gone on backend
    }
    removeTask(task.taskId);
  };

  const handleRedownload = async () => {
    if (!task.cloudEnv) {
      addToast("error", t("errors.unknownError"));
      return;
    }
    const problemNames = verifyResults
      .filter((r) => ["missing", "size-mismatch", "hash-mismatch"].includes(r.status))
      .map((r) => r.fileName);
    if (problemNames.length === 0) return;
    setVerifying(true);
    try {
      const batch = await redownloadFiles(
        task.cloudEnv,
        task.taskId,
        problemNames,
        t("tasks.redownloadBatchName", { name: task.fileName })
      );
      useTaskStore.getState().registerTask(batch.batchId, {
        fileName: batch.batchName,
        cloudEnv: task.cloudEnv,
        driveId: task.driveId,
        homeAccountId: task.homeAccountId,
        type: "download",
      });
      useTaskStore.getState().setVerifyResults(task.taskId, []);
      addToast("success", t("tasks.redownloadStarted", { count: problemNames.length }));
    } catch (err) {
      addToast("error", getErrorMessage(err));
    } finally {
      setVerifying(false);
    }
  };

  const handleVerify = async () => {
    if (!task.cloudEnv) {
      addToast("error", t("errors.unknownError"));
      return;
    }
    setVerifying(true);
    try {
      const results = await verifyBatchFiles(task.cloudEnv, task.taskId);
      useTaskStore.getState().setVerifyResults(task.taskId, results);
    } catch (err) {
      addToast("error", getErrorMessage(err));
    } finally {
      setVerifying(false);
    }
  };

  const verifyLabel = (status: string): string => {
    switch (status) {
      case "verified":
        return t("tasks.verifyVerified");
      case "size-mismatch":
        return t("tasks.verifySizeMismatch");
      case "hash-mismatch":
        return t("tasks.verifyHashMismatch");
      case "missing":
        return t("tasks.verifyMissing");
      case "not-downloaded":
        return t("tasks.verifyNotDownloaded");
      case "hash-unavailable":
        return t("tasks.verifyHashUnavailable");
      default:
        return status;
    }
  };

  const verifyResults = task.verifyResults ?? [];
  const verifyOk = verifyResults.filter((r) => r.status === "verified").length;
  const verifyBad = verifyResults.length - verifyOk;

  return (
    <div className="rounded-lg border border-border bg-card p-3 space-y-2">
      {/* Header: file name + status badge */}
      <div className="flex items-center justify-between gap-2">
        <span className="text-sm font-medium text-foreground truncate flex-1" title={task.fileName}>
          {task.fileName}
        </span>
        <span className={`text-xs px-2 py-0.5 rounded-full font-medium whitespace-nowrap ${getStatusBadgeClasses(task.status)}`}>
          {getStatusLabel(task.status)}
        </span>
      </div>

      {/* Progress bar */}
      <div className="w-full h-2 rounded-full bg-muted overflow-hidden">
        <div
          className={`h-full rounded-full transition-all duration-300 ${
            task.status === "failed"
              ? "bg-red-500"
              : task.status === "completed"
                ? "bg-green-500"
                : "bg-blue-500"
          }`}
          style={{ width: `${progress}%` }}
        />
      </div>

      {/* Stats row: percentage, speed, elapsed time */}
      <div className="flex items-center justify-between text-xs text-muted-foreground">
        <span>{progress}%</span>
        {isActive && <span>{formatSpeed(task.speedBps)}</span>}
        <span>{formatElapsedTime(task.elapsedSecs)}</span>
      </div>

      {/* Error message for failed tasks */}
      {task.status === "failed" && task.error && (
        <p className="text-xs text-red-500" title={task.error}>
          {task.error === "Some files failed to download"
            ? t("tasks.someFilesFailed")
            : task.error}
        </p>
      )}

      {/* Per-file failure details for failed batches */}
      {task.status === "failed" && (task.failedFiles?.length ?? 0) > 0 && (
        <ul className="max-h-28 space-y-1 overflow-y-auto text-xs text-red-500/90">
          {task.failedFiles!.map((file) => (
            <li key={file.fileName} className="break-all" title={file.message}>
              {file.fileName}: {file.message}
            </li>
          ))}
        </ul>
      )}

      {/* Integrity verification results */}
      {verifyResults.length > 0 && (
        <div className="space-y-1">
          <p className="text-xs text-muted-foreground">
            {t("tasks.verifySummary", { ok: verifyOk, bad: verifyBad })}
          </p>
          {verifyBad > 0 && (
            <ul className="max-h-28 space-y-1 overflow-y-auto text-xs text-red-500/90">
              {verifyResults
                .filter((r) => r.status !== "verified")
                .map((r) => (
                  <li key={r.fileName} className="break-all" title={r.detail ?? ""}>
                    {r.fileName}: {verifyLabel(r.status)}
                    {r.detail ? ` — ${r.detail}` : ""}
                  </li>
                ))}
            </ul>
          )}
        </div>
      )}

      {/* Control buttons */}
      <div className="flex items-center gap-1 pt-1">
        {/* Download: downloading → Pause + Cancel */}
        {task.type === "download" && task.status === "downloading" && (
          <>
            <ActionButton
              icon={<Pause className="h-3.5 w-3.5" />}
              label={t("tasks.pause")}
              onClick={handlePause}
            />
            <ActionButton
              icon={<X className="h-3.5 w-3.5" />}
              label={t("tasks.cancel")}
              onClick={handleCancelDownload}
              variant="destructive"
            />
          </>
        )}

        {/* Download: paused → Resume + Cancel */}
        {task.type === "download" && task.status === "paused" && (
          <>
            <ActionButton
              icon={<Play className="h-3.5 w-3.5" />}
              label={t("tasks.resume")}
              onClick={handleResume}
            />
            <ActionButton
              icon={<X className="h-3.5 w-3.5" />}
              label={t("tasks.cancel")}
              onClick={handleCancelDownload}
              variant="destructive"
            />
          </>
        )}

        {/* Download: completed → Open folder + Remove */}
        {task.type === "download" && task.status === "completed" && (
          <>
            <ActionButton
              icon={<FolderOpen className="h-3.5 w-3.5" />}
              label={t("tasks.openFolder")}
              onClick={handleOpenFolder}
            />
            <ActionButton
              icon={<Trash2 className="h-3.5 w-3.5" />}
              label={t("tasks.remove")}
              onClick={handleRemove}
            />
          </>
        )}

        {/* Download: completed/failed → integrity verification */}
        {task.type === "download" &&
         (task.status === "completed" || task.status === "failed") && (
          <ActionButton
            icon={<ShieldCheck className="h-3.5 w-3.5" />}
            label={verifying ? t("tasks.verifying") : t("tasks.verifyFiles")}
            onClick={handleVerify}
            disabled={verifying}
          />
        )}

        {/* Download: verification found problems → re-download them */}
        {task.type === "download" && verifyBad > 0 && !verifying && (
          <ActionButton
            icon={<RefreshCw className="h-3.5 w-3.5" />}
            label={t("tasks.redownloadFiles", { count: verifyBad })}
            onClick={handleRedownload}
          />
        )}

        {/* Download: failed → Resume (from chunk progress) + Remove */}
        {task.type === "download" && task.status === "failed" && (
          <>
            <ActionButton
              icon={<Play className="h-3.5 w-3.5" />}
              label={t("tasks.resume")}
              onClick={handleResume}
            />
            <ActionButton
              icon={<Trash2 className="h-3.5 w-3.5" />}
              label={t("tasks.remove")}
              onClick={handleRemove}
            />
          </>
        )}

        {/* Upload: uploading → Cancel */}
        {task.type === "upload" && task.status === "uploading" && (
          <ActionButton
            icon={<X className="h-3.5 w-3.5" />}
            label={t("tasks.cancel")}
            onClick={handleCancelUpload}
            variant="destructive"
          />
        )}

        {/* Upload: completed → Remove */}
        {task.type === "upload" && task.status === "completed" && (
          <ActionButton
            icon={<Trash2 className="h-3.5 w-3.5" />}
            label={t("tasks.remove")}
            onClick={handleRemove}
          />
        )}

        {/* Upload: failed → Remove */}
        {task.type === "upload" && task.status === "failed" && (
          <ActionButton
            icon={<Trash2 className="h-3.5 w-3.5" />}
            label={t("tasks.remove")}
            onClick={handleRemove}
          />
        )}
      </div>
    </div>
  );
}

/** Small icon button for task actions. */
function ActionButton({
  icon,
  label,
  onClick,
  variant,
  disabled,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  variant?: "destructive";
  disabled?: boolean;
}) {
  const baseClasses =
    "inline-flex items-center gap-1 px-2 py-1 rounded text-xs font-medium transition-colors";
  const variantClasses =
    variant === "destructive"
      ? "text-red-600 hover:bg-red-100 dark:text-red-400 dark:hover:bg-red-900/30"
      : "text-muted-foreground hover:bg-muted hover:text-foreground";

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`${baseClasses} ${variantClasses} ${disabled ? "opacity-50 cursor-not-allowed" : ""}`}
      title={label}
    >
      {icon}
      <span>{label}</span>
    </button>
  );
}
