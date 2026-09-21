import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import type { CloudEnvironment, ProgressEvent, TaskStatus } from "../lib/types";
import { getDownloadTasks } from "../lib/tauri";

/** A single transfer task entry displayed in the UI. */
export interface TaskEntry {
  taskId: string;
  fileName: string;
  status: TaskStatus;
  totalBytes: number;
  transferredBytes: number;
  speedBps: number;
  elapsedSecs: number;
  error: string | null;
  type: "download" | "upload";
  driveId?: string;
  cloudEnv?: CloudEnvironment;
  itemId?: string;
  homeAccountId?: string;
  localPath?: string;
}

interface TaskStoreState {
  tasks: Map<string, TaskEntry>;
  /** When true, the app will initiate system shutdown after all downloads complete. */
  shutdownAfterDownload: boolean;
  updateTask: (event: ProgressEvent) => void;
  registerTask: (taskId: string, metadata: Partial<TaskEntry>) => void;
  loadDownloadTasks: () => Promise<void>;
  removeTask: (taskId: string) => void;
  clearCompleted: (type: "download" | "upload") => void;
  setShutdownAfterDownload: (value: boolean) => void;
}

function determineTaskType(status: TaskStatus): "download" | "upload" {
  if (status === "uploading") return "upload";
  return "download";
}

export const useTaskStore = create<TaskStoreState>((set) => ({
  tasks: new Map(),
  shutdownAfterDownload: false,

  updateTask: (event: ProgressEvent) => {
    set((state) => {
      const newTasks = new Map(state.tasks);
      const existing = newTasks.get(event.taskId);
      const type = existing?.type ?? determineTaskType(event.status);

      newTasks.set(event.taskId, {
        ...existing,
        taskId: event.taskId,
        fileName: event.fileName,
        status: event.status,
        totalBytes: event.totalBytes,
        transferredBytes: event.transferredBytes,
        speedBps: event.speedBps,
        elapsedSecs: event.elapsedSecs,
        error: event.error,
        localPath: event.localPath ?? existing?.localPath,
        type,
      });

      return { tasks: newTasks };
    });
  },

  registerTask: (taskId, metadata) => {
    set((state) => {
      const newTasks = new Map(state.tasks);
      const existing = newTasks.get(taskId);
      const base: TaskEntry = {
        taskId,
        fileName: "",
        status: "queued",
        totalBytes: 0,
        transferredBytes: 0,
        speedBps: 0,
        elapsedSecs: 0,
        error: null,
        type: "download",
      };

      newTasks.set(taskId, {
        ...base,
        ...existing,
        ...metadata,
        taskId,
      });

      return { tasks: newTasks };
    });
  },

  loadDownloadTasks: async () => {
    try {
      const snapshots = await getDownloadTasks();
      set((state) => {
        const newTasks = new Map(state.tasks);
        for (const snapshot of snapshots) {
          newTasks.set(snapshot.id, {
            taskId: snapshot.id,
            fileName: snapshot.name,
            status: snapshot.status,
            totalBytes: snapshot.totalBytes,
            transferredBytes: snapshot.downloadedBytes,
            speedBps: snapshot.speedBps,
            elapsedSecs: snapshot.elapsedSecs,
            error: snapshot.error,
            localPath: snapshot.localPath,
            cloudEnv: snapshot.cloudEnv,
            driveId: snapshot.driveId,
            homeAccountId: snapshot.homeAccountId,
            type: "download",
          });
        }
        return { tasks: newTasks };
      });
    } catch {
      // Restore is best-effort; the task list can start empty if loading fails.
    }
  },

  removeTask: (taskId: string) => {
    set((state) => {
      const newTasks = new Map(state.tasks);
      newTasks.delete(taskId);
      return { tasks: newTasks };
    });
  },

  clearCompleted: (type: "download" | "upload") => {
    set((state) => {
      const newTasks = new Map(state.tasks);
      for (const [id, task] of newTasks) {
        if (task.type === type && task.status === "completed") {
          newTasks.delete(id);
        }
      }
      return { tasks: newTasks };
    });
  },

  setShutdownAfterDownload: (value: boolean) => set({ shutdownAfterDownload: value }),
}));

/**
 * Initialize the Tauri event listener for progress events.
 * Call this once on app startup (e.g., in a useEffect in App.tsx).
 */
export async function initTaskListener() {
  await listen<ProgressEvent>("progress-event", (event) => {
    const raw = event.payload as unknown as Record<string, unknown>;
    const taskId =
      typeof raw.taskId === "string"
        ? raw.taskId
        : typeof raw.task_id === "string"
          ? raw.task_id
          : "";
    if (!taskId) return;

    const normalized: ProgressEvent = {
      taskId,
      fileName:
        typeof raw.fileName === "string"
          ? raw.fileName
          : typeof raw.file_name === "string"
            ? raw.file_name
            : "Unknown",
      status: (raw.status as ProgressEvent["status"]) ?? "queued",
      totalBytes: Number(raw.totalBytes ?? raw.total_bytes ?? 0),
      transferredBytes: Number(raw.transferredBytes ?? raw.transferred_bytes ?? 0),
      speedBps: Number(raw.speedBps ?? raw.speed_bps ?? 0),
      elapsedSecs: Number(raw.elapsedSecs ?? raw.elapsed_secs ?? 0),
      error: typeof raw.error === "string" ? raw.error : null,
      localPath:
        typeof raw.localPath === "string"
          ? raw.localPath
          : typeof raw.local_path === "string"
            ? raw.local_path
            : null,
    };

    useTaskStore.getState().updateTask(normalized);
  });
}
