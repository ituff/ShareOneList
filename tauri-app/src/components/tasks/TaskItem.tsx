import { useTranslation } from "react-i18next";
import { Pause, Play, X, FolderOpen, Trash2 } from "lucide-react";
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
          {task.error}
        </p>
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

        {/* Download: failed → Remove */}
        {task.type === "download" && task.status === "failed" && (
          <ActionButton
            icon={<Trash2 className="h-3.5 w-3.5" />}
            label={t("tasks.remove")}
            onClick={handleRemove}
          />
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
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  variant?: "destructive";
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
      className={`${baseClasses} ${variantClasses}`}
      title={label}
    >
      {icon}
      <span>{label}</span>
    </button>
  );
}
