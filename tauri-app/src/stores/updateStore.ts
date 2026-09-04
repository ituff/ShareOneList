import { create } from "zustand";
import type { UpdateInfo } from "../lib/types";

interface UpdateState {
  /** Latest available update, set by the silent startup check. */
  info: UpdateInfo | null;
  /** Whether the user closed the bubble for this session. */
  dismissed: boolean;
  /** Download state for the "update now" action. */
  downloading: boolean;
  transferred: number;
  total: number;
  error: string | null;

  setInfo: (info: UpdateInfo | null) => void;
  dismiss: () => void;
  setDownloading: (downloading: boolean) => void;
  setProgress: (transferred: number, total: number) => void;
  setError: (error: string | null) => void;
}

export const useUpdateStore = create<UpdateState>((set) => ({
  info: null,
  dismissed: false,
  downloading: false,
  transferred: 0,
  total: 0,
  error: null,

  setInfo: (info) => set({ info, dismissed: false, error: null }),
  dismiss: () => set({ dismissed: true }),
  setDownloading: (downloading) =>
    set({ downloading, transferred: 0, total: 0, error: null }),
  setProgress: (transferred, total) => set({ transferred, total }),
  setError: (error) => set({ error, downloading: false }),
}));
