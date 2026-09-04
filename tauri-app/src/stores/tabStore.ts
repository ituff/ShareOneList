import { create } from "zustand";
import type {
  BreadcrumbItem,
  CloudEnvironment,
  DriveItem,
  LayoutMode,
  TabState,
} from "../lib/types";
import { listFiles } from "../lib/tauri";
import { getErrorMessage } from "../lib/errors";
import { isAuthError } from "../lib/errors";
import { useAuthStore } from "./authStore";

/** Maximum number of simultaneous tabs allowed. */
const MAX_TABS = 10;

/** Maximum items to display before showing "Load more" pagination. */
const PAGE_SIZE = 200;

interface TabStoreState {
  /** All open tabs. */
  tabs: TabState[];
  /** ID of the currently active tab, or null if no tabs are open. */
  activeTabId: string | null;

  // ─── Tab Lifecycle ──────────────────────────────────────────────────────────

  /**
   * Open a tab for the given drive.
   * If a tab for that driveId already exists, switch to it.
   * Otherwise create a new tab (up to MAX_TABS). If max reached, do nothing.
   */
  openTab: (
    driveId: string,
    driveName: string,
    cloudEnv: CloudEnvironment,
    homeAccountId: string
  ) => void;

  /**
   * Open a preview tab for a file.
   * If a preview tab for that item already exists, switch to it.
   */
  openPreviewTab: (
    item: DriveItem,
    driveId: string,
    cloudEnv: CloudEnvironment,
    homeAccountId: string
  ) => void;

  /**
   * Open the meeting-recordings list tab for an account.
   * Reuses the existing tab for the same account instead of duplicating.
   */
  openRecordingsTab: (
    homeAccountId: string,
    cloudEnv: CloudEnvironment,
    title: string
  ) => void;

  /** Open a new tab at a specific folder. */
  openTabAtFolder: (
    driveId: string,
    driveName: string,
    cloudEnv: CloudEnvironment,
    homeAccountId: string,
    folderId: string,
    folderName: string
  ) => void;

  /** Close a tab by ID. Switch to nearest remaining tab (prefer right, then left). */
  closeTab: (tabId: string) => void;

  /** Switch to a specific tab by ID. */
  switchTab: (tabId: string) => void;

  // ─── Per-Tab State Updates ──────────────────────────────────────────────────

  /** Update partial state for a specific tab. */
  updateTabState: (tabId: string, updates: Partial<TabState>) => void;

  // ─── Per-Tab Navigation ─────────────────────────────────────────────────────

  /** Load folder contents for a specific tab. */
  loadFolder: (tabId: string, folderId: string) => Promise<void>;

  /** Navigate into a subfolder within the active tab. */
  navigateToFolder: (tabId: string, folderId: string, folderName: string) => void;

  /** Navigate to a breadcrumb by index within a tab. */
  navigateToBreadcrumb: (tabId: string, index: number) => void;

  /** Navigate a tab up one directory level. */
  navigateUp: (tabId: string) => void;

  /** Navigate a tab back to its root folder. */
  navigateToRoot: (tabId: string) => void;

  /** Set the layout mode for a tab. */
  setTabLayoutMode: (tabId: string, mode: LayoutMode) => void;
}

/** Generate a unique tab ID. */
function generateTabId(): string {
  return `tab-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

/** Sort items: folders first, then files, alphabetical within each group (case-insensitive). */
function sortItems(items: DriveItem[]): DriveItem[] {
  return [...items].sort((a, b) => {
    if (a.isFolder && !b.isFolder) return -1;
    if (!a.isFolder && b.isFolder) return 1;
    return a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
  });
}

export const useTabStore = create<TabStoreState>((set, get) => ({
  tabs: [],
  activeTabId: null,

  openTab: (driveId, driveName, cloudEnv, homeAccountId) => {
    const { tabs } = get();

    // If a tab for this drive already exists, switch to it
    const existing = tabs.find((t) => t.kind === "drive" && t.driveId === driveId);
    if (existing) {
      set({ activeTabId: existing.id });
      return;
    }

    // If max tabs reached, do nothing
    if (tabs.length >= MAX_TABS) {
      return;
    }

    // Create new tab
    const newTab: TabState = {
      id: generateTabId(),
      kind: "drive",
      driveId,
      driveName,
      cloudEnv,
      homeAccountId,
      currentFolderId: "root",
      breadcrumbs: [],
      items: [],
      layoutMode: "list",
      isLoading: true,
      error: null,
    };

    set({
      tabs: [...tabs, newTab],
      activeTabId: newTab.id,
    });

    // Load root folder contents
    get().loadFolder(newTab.id, "root");
  },

  openPreviewTab: (item, driveId, cloudEnv, homeAccountId) => {
    const { tabs } = get();
    const existing = tabs.find((t) => t.kind === "preview" && t.previewItem?.id === item.id);
    if (existing) {
      set({ activeTabId: existing.id });
      return;
    }

    const newTab: TabState = {
      id: generateTabId(),
      kind: "preview",
      driveId,
      driveName: item.name,
      cloudEnv,
      homeAccountId,
      previewItem: item,
      currentFolderId: "root",
      breadcrumbs: [],
      items: [],
      layoutMode: "list",
      isLoading: false,
      error: null,
    };

    set({
      tabs: [...tabs, newTab],
      activeTabId: newTab.id,
    });
  },

  openRecordingsTab: (homeAccountId, cloudEnv, title) => {
    const { tabs } = get();

    // One recordings tab per account.
    const existing = tabs.find(
      (t) => t.kind === "recordings" && t.homeAccountId === homeAccountId
    );
    if (existing) {
      set({ activeTabId: existing.id });
      return;
    }
    if (tabs.length >= MAX_TABS) {
      return;
    }

    const newTab: TabState = {
      id: generateTabId(),
      kind: "recordings",
      driveId: "",
      driveName: title,
      cloudEnv,
      homeAccountId,
      currentFolderId: "root",
      breadcrumbs: [],
      items: [],
      layoutMode: "list",
      isLoading: false,
      error: null,
    };

    set({
      tabs: [...tabs, newTab],
      activeTabId: newTab.id,
    });
  },

  openTabAtFolder: (driveId, driveName, cloudEnv, homeAccountId, folderId, folderName) => {
    get().openTab(driveId, driveName, cloudEnv, homeAccountId);
    const activeTabId = get().activeTabId;
    if (activeTabId) {
      get().navigateToFolder(activeTabId, folderId, folderName);
    }
  },

  closeTab: (tabId) => {
    const { tabs, activeTabId } = get();
    const index = tabs.findIndex((t) => t.id === tabId);
    if (index === -1) return;

    const newTabs = tabs.filter((t) => t.id !== tabId);

    // If the closed tab was active, switch to nearest (prefer right, then left)
    let newActiveId: string | null = activeTabId;
    if (activeTabId === tabId) {
      if (newTabs.length === 0) {
        newActiveId = null;
      } else if (index < newTabs.length) {
        // Right neighbor exists (same index in new array)
        newActiveId = newTabs[index].id;
      } else {
        // Left neighbor (last in new array)
        newActiveId = newTabs[newTabs.length - 1].id;
      }
    }

    set({ tabs: newTabs, activeTabId: newActiveId });
  },

  switchTab: (tabId) => {
    const { tabs } = get();
    if (tabs.some((t) => t.id === tabId)) {
      set({ activeTabId: tabId });
    }
  },

  updateTabState: (tabId, updates) => {
    set({
      tabs: get().tabs.map((t) =>
        t.id === tabId ? { ...t, ...updates } : t
      ),
    });
  },

  loadFolder: async (tabId, folderId) => {
    const { tabs } = get();
    const tab = tabs.find((t) => t.id === tabId);
    if (!tab) return;

    // Set loading state
    get().updateTabState(tabId, {
      isLoading: true,
      currentFolderId: folderId,
      error: null,
    });

    try {
      const rawItems = await listFiles(tab.driveId, folderId, tab.cloudEnv);
      const sorted = sortItems(rawItems);
      const visible = sorted.slice(0, PAGE_SIZE);
      get().updateTabState(tabId, {
        items: visible,
        isLoading: false,
      });
    } catch (err) {
      if (isAuthError(err)) {
        useAuthStore.getState().setPendingRelogin({
          cloudEnv: tab.cloudEnv,
          tabId,
          folderId,
        });
      }
      get().updateTabState(tabId, {
        items: [],
        isLoading: false,
        error: getErrorMessage(err, "Failed to load folder"),
      });
    }
  },

  navigateToFolder: (tabId, folderId, folderName) => {
    const { tabs } = get();
    const tab = tabs.find((t) => t.id === tabId);
    if (!tab) return;

    const newBreadcrumbs: BreadcrumbItem[] = [
      ...tab.breadcrumbs,
      { id: folderId, name: folderName },
    ];
    get().updateTabState(tabId, { breadcrumbs: newBreadcrumbs });
    get().loadFolder(tabId, folderId);
  },

  navigateToBreadcrumb: (tabId, index) => {
    const { tabs } = get();
    const tab = tabs.find((t) => t.id === tabId);
    if (!tab || index < 0 || index >= tab.breadcrumbs.length) return;

    const target = tab.breadcrumbs[index];
    const trimmed = tab.breadcrumbs.slice(0, index + 1);
    get().updateTabState(tabId, { breadcrumbs: trimmed });
    get().loadFolder(tabId, target.id);
  },

  navigateUp: (tabId) => {
    const { tabs } = get();
    const tab = tabs.find((t) => t.id === tabId);
    if (!tab || tab.kind !== "drive") return;

    if (tab.breadcrumbs.length === 0) {
      return;
    }
    if (tab.breadcrumbs.length === 1) {
      get().updateTabState(tabId, { breadcrumbs: [] });
      get().loadFolder(tabId, "root");
      return;
    }
    get().navigateToBreadcrumb(tabId, tab.breadcrumbs.length - 2);
  },

  navigateToRoot: (tabId) => {
    const { tabs } = get();
    const tab = tabs.find((t) => t.id === tabId);
    if (!tab || tab.kind !== "drive") return;
    get().updateTabState(tabId, {
      breadcrumbs: [],
    });
    get().loadFolder(tabId, "root");
  },

  setTabLayoutMode: (tabId, mode) => {
    get().updateTabState(tabId, { layoutMode: mode });
  },
}));
