import { useTranslation } from "react-i18next";
import { useTaskStore, type TaskEntry } from "../../stores/taskStore";
import { TaskItem } from "./TaskItem";
import { Pause, Play, Power } from "lucide-react";
import { pauseDownload, resumeDownload } from "../../lib/tauri";
import { useToastStore } from "../../stores/toastStore";
import { useAuthStore } from "../../stores/authStore";
import { getErrorMessage, isAuthError } from "../../lib/errors";

export function TaskManager() {
  const { t } = useTranslation();
  const tasks = useTaskStore((s) => s.tasks);
  const clearCompleted = useTaskStore((s) => s.clearCompleted);
  const shutdownAfterDownload = useTaskStore((s) => s.shutdownAfterDownload);
  const setShutdownAfterDownload = useTaskStore((s) => s.setShutdownAfterDownload);
  const addToast = useToastStore((s) => s.addToast);

  // Split tasks into downloads and uploads
  const downloads: TaskEntry[] = [];
  const uploads: TaskEntry[] = [];

  for (const task of tasks.values()) {
    if (task.type === "download") {
      downloads.push(task);
    } else {
      uploads.push(task);
    }
  }

  const hasCompletedDownloads = downloads.some((t) => t.status === "completed");
  const hasCompletedUploads = uploads.some((t) => t.status === "completed");
  const hasPausedDownloads = downloads.some(
    (t) => t.status === "paused" && Boolean(t.cloudEnv)
  );
  const hasActiveDownloads = downloads.some((t) => t.status === "downloading");

  const handleResumeAll = async () => {
    for (const task of downloads) {
      if (task.status !== "paused" || !task.cloudEnv) continue;
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
    }
  };

  const handlePauseAll = async () => {
    for (const task of downloads) {
      if (task.status !== "downloading") continue;
      try {
        await pauseDownload(task.taskId);
      } catch (err) {
        addToast("error", getErrorMessage(err));
      }
    }
  };

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-foreground">{t("tasks.title")}</h2>
        <p className="text-muted-foreground">{t("tasks.description")}</p>
      </div>

      {/* Downloads Section */}
      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-lg font-semibold text-foreground">
            {t("tasks.downloads")} ({downloads.length})
          </h3>
          <div className="flex items-center gap-3">
            {hasActiveDownloads && (
              <button
                onClick={handlePauseAll}
                className="inline-flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
              >
                <Pause className="h-3 w-3" />
                {t("tasks.pauseAll")}
              </button>
            )}
            {hasPausedDownloads && (
              <button
                onClick={handleResumeAll}
                className="inline-flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
              >
                <Play className="h-3 w-3" />
                {t("tasks.resumeAll")}
              </button>
            )}
            {/* Shutdown after download toggle */}
            <label className="flex items-center gap-1.5 text-xs text-muted-foreground cursor-pointer select-none">
              <input
                type="checkbox"
                checked={shutdownAfterDownload}
                onChange={(e) => setShutdownAfterDownload(e.target.checked)}
                className="h-3.5 w-3.5 rounded border-border accent-primary"
              />
              <Power className="h-3 w-3" />
              <span>{t("tasks.shutdownAfterDownload")}</span>
            </label>
            {hasCompletedDownloads && (
              <button
                onClick={() => clearCompleted("download")}
                className="text-xs text-muted-foreground hover:text-foreground transition-colors"
              >
                {t("tasks.clearCompleted")}
              </button>
            )}
          </div>
        </div>

        {downloads.length === 0 ? (
          <p className="text-sm text-muted-foreground italic">{t("tasks.noTasks")}</p>
        ) : (
          <div className="space-y-2">
            {downloads.map((task) => (
              <TaskItem key={task.taskId} task={task} />
            ))}
          </div>
        )}
      </section>

      {/* Uploads Section */}
      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-lg font-semibold text-foreground">
            {t("tasks.uploads")} ({uploads.length})
          </h3>
          {hasCompletedUploads && (
            <button
              onClick={() => clearCompleted("upload")}
              className="text-xs text-muted-foreground hover:text-foreground transition-colors"
            >
              {t("tasks.clearCompleted")}
            </button>
          )}
        </div>

        {uploads.length === 0 ? (
          <p className="text-sm text-muted-foreground italic">{t("tasks.noTasks")}</p>
        ) : (
          <div className="space-y-2">
            {uploads.map((task) => (
              <TaskItem key={task.taskId} task={task} />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
