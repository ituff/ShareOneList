import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import type { ProgressEvent, TaskStatus } from "../lib/types";

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
}

interface TaskStoreState {
  tasks: Map<string, TaskEntry>;
  /** When true, the app will initiate system shutdown after all downloads complete. */
  shutdownAfterDownload: boolean;
  updateTask: (event: ProgressEvent) => void;
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
        taskId: event.taskId,
        fileName: event.fileName,
        status: event.status,
        totalBytes: event.totalBytes,
        transferredBytes: event.transferredBytes,
        speedBps: event.speedBps,
        elapsedSecs: event.elapsedSecs,
        error: event.error,
        type,
      });

      return { tasks: newTasks };
    });
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
    useTaskStore.getState().updateTask(event.payload);
  });
}
