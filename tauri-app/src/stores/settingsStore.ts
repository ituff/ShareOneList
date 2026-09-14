import { create } from "zustand";

/** Sub-tab within the settings page ("ai" is deep-link target for memory editing). */
export type SettingsTab = "appearance" | "downloads" | "ai" | "backup" | "about";
/** Update channel: stable releases only, or prereleases included. */
export type UpdateChannel = "stable" | "beta";
import type { AppConfig, ThemeMode, WindowState } from "../lib/types";
import { getConfig, saveConfig } from "../lib/tauri";

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
  /** Directory receiving automatic config backups; null disables. */
  autoBackupDir: string | null;
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
  /** Set the auto backup directory (null disables) and persist. */
  setAutoBackupDir: (dir: string | null) => void;
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
 * Persist the current config to the backend.
 * Fire-and-forget for UI responsiveness — errors are logged but don't block the UI.
 */
function persistConfig(config: AppConfig): void {
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
  autoBackupDir: null,
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
        autoBackupDir: config.autoBackupDir ?? null,
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
    const state = get();
    persistConfig({
      theme: mode,
      language: state.language,
      window: state.window,
      lastDownloadPath: state.lastDownloadPath,
      segmentDownloadConcurrency: state.segmentDownloadConcurrency,
      updateChannel: state.updateChannel,
      autoBackupDir: state.autoBackupDir,
    });
  },

  setLanguage: (lang) => {
    set({ language: lang });
    const state = get();
    persistConfig({
      theme: state.theme,
      language: lang,
      window: state.window,
      lastDownloadPath: state.lastDownloadPath,
      segmentDownloadConcurrency: state.segmentDownloadConcurrency,
      updateChannel: state.updateChannel,
      autoBackupDir: state.autoBackupDir,
    });
  },

  setWindowState: (windowState) => {
    set({ window: windowState });
    const state = get();
    persistConfig({
      theme: state.theme,
      language: state.language,
      window: windowState,
      lastDownloadPath: state.lastDownloadPath,
      segmentDownloadConcurrency: state.segmentDownloadConcurrency,
      updateChannel: state.updateChannel,
      autoBackupDir: state.autoBackupDir,
    });
  },

  setLastDownloadPath: (path) => {
    set({ lastDownloadPath: path });
    const state = get();
    persistConfig({
      theme: state.theme,
      language: state.language,
      window: state.window,
      lastDownloadPath: path,
      segmentDownloadConcurrency: state.segmentDownloadConcurrency,
      updateChannel: state.updateChannel,
      autoBackupDir: state.autoBackupDir,
    });
  },

  setAutoBackupDir: (dir) => {
    set({ autoBackupDir: dir });
    const state = get();
    persistConfig({
      theme: state.theme,
      language: state.language,
      window: state.window,
      lastDownloadPath: state.lastDownloadPath,
      segmentDownloadConcurrency: state.segmentDownloadConcurrency,
      updateChannel: state.updateChannel,
      autoBackupDir: dir,
    });
  },

  setSegmentDownloadConcurrency: (n) => {
    const clamped = Math.min(16, Math.max(1, Math.round(n) || 4));
    set({ segmentDownloadConcurrency: clamped });
    const state = get();
    persistConfig({
      theme: state.theme,
      language: state.language,
      window: state.window,
      lastDownloadPath: state.lastDownloadPath,
      segmentDownloadConcurrency: clamped,
      updateChannel: state.updateChannel,
      autoBackupDir: state.autoBackupDir,
    });
  },

  setUpdateChannel: (channel) => {
    set({ updateChannel: channel });
    const state = get();
    persistConfig({
      theme: state.theme,
      language: state.language,
      window: state.window,
      lastDownloadPath: state.lastDownloadPath,
      segmentDownloadConcurrency: state.segmentDownloadConcurrency,
      updateChannel: channel,
      autoBackupDir: state.autoBackupDir,
    });
  },

  setSettingsTab: (tab) => {
    set({ settingsTab: tab });
  },
}));
