import { create } from "zustand";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { UpdateInfo } from "../lib/types";

interface UpdateState {
  /** Latest available update, set by the silent startup check. */
  info: UpdateInfo | null;
  /** Whether the user closed the bubble for this session. */
  dismissed: boolean;
  /** Download state for the "update now" action (shared by bubble and settings). */
  downloading: boolean;
  transferred: number;
  total: number;
  /** Rolling-window download speed in bytes per second (0 while unknown). */
  speed: number;
  error: string | null;

  setInfo: (info: UpdateInfo | null) => void;
  dismiss: () => void;
  reopen: () => void;
  setDownloading: (downloading: boolean) => void;
  setProgress: (transferred: number, total: number, speed: number) => void;
  setError: (error: string | null) => void;
}

export const useUpdateStore = create<UpdateState>((set) => ({
  info: null,
  dismissed: false,
  downloading: false,
  transferred: 0,
  total: 0,
  speed: 0,
  error: null,

  setInfo: (info) => set({ info, dismissed: false, error: null }),
  dismiss: () => set({ dismissed: true }),
  reopen: () => set({ dismissed: false }),
  setDownloading: (downloading) =>
    set({ downloading, transferred: 0, total: 0, speed: 0, error: null }),
  setProgress: (transferred, total, speed) => set({ transferred, total, speed }),
  setError: (error) => set({ error, downloading: false }),
}));

/**
 * Initialize the Tauri event listener for update download progress.
 * Call this once on app startup (e.g., in a useEffect in App.tsx) so progress
 * keeps flowing into the store no matter which components are mounted.
 */
let unlistenProgress: UnlistenFn | undefined;
export async function initUpdateProgressListener() {
  if (unlistenProgress) return;
  unlistenProgress = await listen<{
    transferred: number;
    total: number;
    speed?: number;
  }>("update-download-progress", (event) => {
    useUpdateStore.getState().setProgress(
      event.payload.transferred,
      event.payload.total,
      event.payload.speed ?? 0
    );
  });
}
