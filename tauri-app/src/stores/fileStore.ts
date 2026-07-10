import { create } from "zustand";
import type { BreadcrumbItem, CloudEnvironment, DriveItem, LayoutMode } from "../lib/types";
import { listFiles } from "../lib/tauri";

/** Maximum items to display before showing "Load more" pagination. */
const PAGE_SIZE = 200;

interface FileState {
  /** Current drive ID being browsed. */
  driveId: string | null;
  /** Current folder ID. */
  currentFolderId: string | null;
  /** Current cloud environment. */
  currentCloudEnv: CloudEnvironment | null;
  /** Visible items in the current folder (paginated). */
  items: DriveItem[];
  /** All items loaded from the API (before pagination). */
  allItems: DriveItem[];
  /** Breadcrumb trail from root to current folder. */
  breadcrumbs: BreadcrumbItem[];
  /** Whether a file listing request is in progress. */
  isLoading: boolean;
  /** Error message from the last failed request. */
  error: string | null;
  /** Current layout mode. */
  layoutMode: LayoutMode;
  /** Number of items currently shown (for pagination). */
  visibleCount: number;
  /** Whether there are more items to show. */
  hasMore: boolean;

  /** Load the contents of a folder. */
  loadFolder: (driveId: string, folderId: string, cloudEnv: CloudEnvironment) => Promise<void>;
  /** Navigate to a breadcrumb item by index (trims trail and loads folder). */
  navigateToBreadcrumb: (index: number) => void;
  /** Navigate to the parent folder (go up one level). */
  navigateUp: () => void;
  /** Navigate into a subfolder. */
  navigateToFolder: (folderId: string, folderName: string) => void;
  /** Load more items (pagination). */
  loadMore: () => void;
  /** Set layout mode. */
  setLayoutMode: (mode: LayoutMode) => void;
  /** Initialize browsing session for a drive. */
  initDrive: (driveId: string, driveName: string, rootFolderId: string, cloudEnv: CloudEnvironment) => void;
  /** Clear the current error. */
  clearError: () => void;
  /** Reset all state. */
  reset: () => void;
}

/** Sort items: folders first, then files, alphabetical within each group (case-insensitive). */
function sortItems(items: DriveItem[]): DriveItem[] {
  return [...items].sort((a, b) => {
    if (a.isFolder && !b.isFolder) return -1;
    if (!a.isFolder && b.isFolder) return 1;
    return a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
  });
}

export const useFileStore = create<FileState>((set, get) => ({
  driveId: null,
  currentFolderId: null,
  currentCloudEnv: null,
  items: [],
  allItems: [],
  breadcrumbs: [],
  isLoading: false,
  error: null,
  layoutMode: "list",
  visibleCount: PAGE_SIZE,
  hasMore: false,

  loadFolder: async (driveId, folderId, cloudEnv) => {
    set({ isLoading: true, error: null });
    try {
      const rawItems = await listFiles(driveId, folderId, cloudEnv);
      const sorted = sortItems(rawItems);
      const visibleCount = Math.min(PAGE_SIZE, sorted.length);
      set({
        allItems: sorted,
        items: sorted.slice(0, visibleCount),
        driveId,
        currentFolderId: folderId,
        currentCloudEnv: cloudEnv,
        isLoading: false,
        visibleCount,
        hasMore: sorted.length > PAGE_SIZE,
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to load folder";
      set({ isLoading: false, error: message, items: [], allItems: [] });
    }
  },

  navigateToBreadcrumb: (index) => {
    const { breadcrumbs, driveId, currentCloudEnv } = get();
    if (index < 0 || index >= breadcrumbs.length || !driveId || !currentCloudEnv) return;

    const target = breadcrumbs[index];
    // Trim breadcrumbs to the selected index (inclusive)
    set({ breadcrumbs: breadcrumbs.slice(0, index + 1), visibleCount: PAGE_SIZE });
    get().loadFolder(driveId, target.id, currentCloudEnv);
  },

  navigateUp: () => {
    const { breadcrumbs } = get();
    if (breadcrumbs.length <= 1) return; // Already at root
    get().navigateToBreadcrumb(breadcrumbs.length - 2);
  },

  navigateToFolder: (folderId, folderName) => {
    const { breadcrumbs, driveId, currentCloudEnv } = get();
    if (!driveId || !currentCloudEnv) return;

    const newBreadcrumbs = [...breadcrumbs, { id: folderId, name: folderName }];
    set({ breadcrumbs: newBreadcrumbs, visibleCount: PAGE_SIZE });
    get().loadFolder(driveId, folderId, currentCloudEnv);
  },

  loadMore: () => {
    const { allItems, visibleCount } = get();
    const newCount = Math.min(visibleCount + PAGE_SIZE, allItems.length);
    set({
      items: allItems.slice(0, newCount),
      visibleCount: newCount,
      hasMore: newCount < allItems.length,
    });
  },

  setLayoutMode: (mode) => set({ layoutMode: mode }),

  initDrive: (driveId, driveName, rootFolderId, cloudEnv) => {
    set({
      driveId,
      currentFolderId: rootFolderId,
      currentCloudEnv: cloudEnv,
      breadcrumbs: [{ id: rootFolderId, name: driveName }],
      items: [],
      allItems: [],
      error: null,
      visibleCount: PAGE_SIZE,
      hasMore: false,
    });
    get().loadFolder(driveId, rootFolderId, cloudEnv);
  },

  clearError: () => set({ error: null }),

  reset: () =>
    set({
      driveId: null,
      currentFolderId: null,
      currentCloudEnv: null,
      items: [],
      allItems: [],
      breadcrumbs: [],
      isLoading: false,
      error: null,
      layoutMode: "list",
      visibleCount: PAGE_SIZE,
      hasMore: false,
    }),
}));
