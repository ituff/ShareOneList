import { create } from "zustand";
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
  isLoaded: false,

  loadConfig: async () => {
    try {
      const config = await getConfig();
      set({
        theme: config.theme,
        language: config.language,
        window: config.window,
        lastDownloadPath: config.lastDownloadPath ?? null,
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
    });
  },
}));
