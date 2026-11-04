import { create } from "zustand";

/** Sub-tab within the settings page ("ai" is deep-link target for memory editing). */
export type SettingsTab = "appearance" | "general" | "downloads" | "ai" | "backup" | "mount" | "about";
/** Update channel: stable releases only, or prereleases included. */
export type UpdateChannel = "stable" | "beta";
import type {
  AppConfig,
  AutoBackupTarget,
  BackupItem,
  ThemeMode,
  WindowState,
} from "../lib/types";
import { getConfig, saveConfig } from "../lib/tauri";

/** Backup sections enabled by default (matches the Rust default). */
export const defaultBackupItems: BackupItem[] = [
  "tokens",
  "llm_config",
  "llm_keys",
  "chat_history",
  "webdav_mounts",
];

interface SettingsState {
  /** Current theme mode preference. */
  theme: ThemeMode;
  /** Current language code (e.g. "en-US", "zh-CN"). */
  language: string;
  /** Current window position and dimensions. */
  window: WindowState;
  /** Last directory used for downloads. */
  lastDownloadPath: string | null;
  /** Concurrent segment fetches for the recording stream pipeline. */
  segmentDownloadConcurrency: number;
  /** Update channel: "stable" or "beta" (prereleases included). */
  updateChannel: UpdateChannel;
  /** Cloud OneDrive folder receiving automatic config backups; null disables.
   * Changed only through `setAutoBackupTarget` in lib/tauri.ts (which also
   * uploads the first backup) followed by `loadConfig()`. */
  autoBackupTarget: AutoBackupTarget | null;
  /** Optional data sections included in backups. */
  autoBackupItems: BackupItem[];
  /** Closing the main window hides it to the tray instead of exiting. */
  closeToTray: boolean;
  /** Whether the initial config has been loaded from the backend. */
  isLoaded: boolean;

  /** Fetch config from the backend and update local state. */
  loadConfig: () => Promise<void>;
  /** Update the theme mode and persist to backend. */
  setTheme: (mode: ThemeMode) => void;
  /** Update the language and persist to backend. */
  setLanguage: (lang: string) => void;
  /** Update the window state and persist to backend. */
  setWindowState: (state: WindowState) => void;
  /** Update the last used download directory and persist to backend. */
  setLastDownloadPath: (path: string | null) => void;
  /** Update the recording segment download concurrency and persist to backend. */
  setSegmentDownloadConcurrency: (n: number) => void;
  /** Switch the update channel and persist to backend. */
  setUpdateChannel: (channel: UpdateChannel) => void;
  /** Toggle close-to-tray and persist to backend. */
  setCloseToTray: (value: boolean) => void;
  /** Update which optional sections are backed up, and persist. */
  setAutoBackupItems: (items: BackupItem[]) => void;
  /** Active sub-tab in the settings page. */
  settingsTab: SettingsTab;
  setSettingsTab: (tab: SettingsTab) => void;
}

/** Default window state used before config is loaded. */
const defaultWindow: WindowState = {
  x: 0,
  y: 0,
  width: 1280,
  height: 720,
  isMaximized: false,
};

/**
 * Persist the store's current state to the backend. Call after `set()` so
 * `get()` already reflects the change — setters stay one-liners.
 * Fire-and-forget for UI responsiveness — errors are logged but don't block.
 */
function persistCurrent(state: SettingsState): void {
  const config: AppConfig = {
    theme: state.theme,
    language: state.language,
    window: state.window,
    lastDownloadPath: state.lastDownloadPath,
    segmentDownloadConcurrency: state.segmentDownloadConcurrency,
    updateChannel: state.updateChannel,
    autoBackupTarget: state.autoBackupTarget,
    autoBackupItems: state.autoBackupItems,
    closeToTray: state.closeToTray,
  };
  saveConfig(config).catch((err) => {
    console.error("[settingsStore] Failed to persist config:", err);
  });
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  theme: "system",
  language: "system",
  window: defaultWindow,
  lastDownloadPath: null,
  segmentDownloadConcurrency: 4,
  updateChannel: "stable",
  autoBackupTarget: null,
  autoBackupItems: defaultBackupItems,
  closeToTray: false,
  isLoaded: false,
  settingsTab: "appearance",

  loadConfig: async () => {
    try {
      const config = await getConfig();
      set({
        theme: config.theme,
        language: config.language,
        window: config.window,
        lastDownloadPath: config.lastDownloadPath ?? null,
        segmentDownloadConcurrency: config.segmentDownloadConcurrency ?? 4,
        updateChannel:
          config.updateChannel === "beta" ? "beta" : "stable",
        autoBackupTarget: config.autoBackupTarget ?? null,
        autoBackupItems:
          config.autoBackupItems && config.autoBackupItems.length > 0
            ? config.autoBackupItems
            : [],
        closeToTray: config.closeToTray ?? false,
        isLoaded: true,
      });
    } catch (err) {
      console.error("[settingsStore] Failed to load config:", err);
      // Mark as loaded even on failure so the app can proceed with defaults
      set({ isLoaded: true });
    }
  },

  setTheme: (mode) => {
    set({ theme: mode });
    persistCurrent(get());
  },

  setLanguage: (lang) => {
    set({ language: lang });
    persistCurrent(get());
  },

  setWindowState: (windowState) => {
    set({ window: windowState });
    persistCurrent(get());
  },

  setLastDownloadPath: (path) => {
    set({ lastDownloadPath: path });
    persistCurrent(get());
  },

  setSegmentDownloadConcurrency: (n) => {
    const clamped = Math.min(16, Math.max(1, Math.round(n) || 4));
    set({ segmentDownloadConcurrency: clamped });
    persistCurrent(get());
  },

  setUpdateChannel: (channel) => {
    set({ updateChannel: channel });
    persistCurrent(get());
  },

  setCloseToTray: (value) => {
    set({ closeToTray: value });
    persistCurrent(get());
  },

  setAutoBackupItems: (items) => {
    set({ autoBackupItems: items });
    persistCurrent(get());
  },

  setSettingsTab: (tab) => {
    set({ settingsTab: tab });
  },
}));
