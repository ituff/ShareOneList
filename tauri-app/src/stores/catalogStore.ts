import { create } from "zustand";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface CatalogEventPayload {
  accountId: string;
  driveId: string;
  /** 'seeding' | 'ready' | 'failed' | 'indexing' | 'cancelled' */
  status: string;
  visitedNodes: number;
  currentPath: string;
}

interface CatalogState {
  /** Latest event per drive key "accountId|driveId". */
  progress: Record<string, CatalogEventPayload>;
  /** Subscribe to catalog-event; returns the unlisten function. */
  subscribe: () => Promise<UnlistenFn>;
}

export const CATALOG_EVENT = "catalog-event";

export const useCatalogStore = create<CatalogState>((set) => ({
  progress: {},
  subscribe: async () =>
    listen<CatalogEventPayload>(CATALOG_EVENT, (event) => {
      const payload = event.payload;
      const key = `${payload.accountId}|${payload.driveId}`;
      set((state) => ({
        progress: { ...state.progress, [key]: payload },
      }));
    }),
}));

/** Progress for a drive key (or undefined when idle). */
export function driveProgress(
  progress: Record<string, CatalogEventPayload>,
  accountId: string,
  driveId: string
): CatalogEventPayload | undefined {
  return progress[`${accountId}|${driveId}`];
}
