/**
 * Hook for managing window state persistence and restoration.
 *
 * On mount:
 *   1. Loads saved config from backend
 *   2. Validates saved position is within display bounds
 *   3. Sets window position/size using Tauri window API
 *
 * During runtime:
 *   - Listens for window resize/move events
 *   - Debounces (500ms) and saves updated state to backend config
 *
 * Off-screen handling:
 *   - If saved position is outside all connected monitors, centers on primary display
 */

import { useEffect, useRef } from "react";
import { getCurrentWindow, PhysicalPosition, PhysicalSize } from "@tauri-apps/api/window";
import { availableMonitors, currentMonitor } from "@tauri-apps/api/window";
import { getConfig } from "../lib/tauri";
import type { AppConfig, WindowState } from "../lib/types";
import { useSettingsStore } from "../stores/settingsStore";

/** Default window dimensions matching tauri.conf.json */
const DEFAULT_WIDTH = 1024;
const DEFAULT_HEIGHT = 768;

/** Debounce delay for persisting window state changes (ms) */
const SAVE_DEBOUNCE_MS = 500;

/**
 * Check if a window position is within the bounds of any connected monitor.
 * A position is considered "on-screen" if at least part of the window (top-left corner)
 * falls within any monitor's bounds.
 */
function isPositionOnScreen(
  x: number,
  y: number,
  monitors: Array<{ position: { x: number; y: number }; size: { width: number; height: number } }>
): boolean {
  if (monitors.length === 0) return false;

  return monitors.some((monitor) => {
    const monLeft = monitor.position.x;
    const monTop = monitor.position.y;
    const monRight = monLeft + monitor.size.width;
    const monBottom = monTop + monitor.size.height;

    // Window top-left must be within the monitor bounds (with some margin)
    return x >= monLeft && x < monRight && y >= monTop && y < monBottom;
  });
}

/**
 * Calculate a centered position on the primary monitor for a given window size.
 */
function getCenteredPosition(
  width: number,
  height: number,
  primaryMonitor: { position: { x: number; y: number }; size: { width: number; height: number } } | null
): { x: number; y: number } {
  if (!primaryMonitor) {
    return { x: 0, y: 0 };
  }

  const x = primaryMonitor.position.x + Math.round((primaryMonitor.size.width - width) / 2);
  const y = primaryMonitor.position.y + Math.round((primaryMonitor.size.height - height) / 2);
  return { x, y };
}

export function useWindowState(): void {
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const configRef = useRef<AppConfig | null>(null);

  useEffect(() => {
    const appWindow = getCurrentWindow();
    let unlisten1: (() => void) | null = null;
    let unlisten2: (() => void) | null = null;

    async function restoreWindowState() {
      try {
        // Load config from backend
        const config = await getConfig();
        configRef.current = config;
        useSettingsStore.setState({
          theme: config.theme,
          language: config.language,
          window: config.window,
          lastDownloadPath: config.lastDownloadPath ?? null,
          isLoaded: true,
        });
        const windowState = config.window;

        // Get available monitors for bounds checking
        const monitors = await availableMonitors();
        const primary = await currentMonitor();

        const monitorData = monitors.map((m) => ({
          position: { x: m.position.x, y: m.position.y },
          size: { width: m.size.width, height: m.size.height },
        }));

        const width = windowState.width || DEFAULT_WIDTH;
        const height = windowState.height || DEFAULT_HEIGHT;

        // Determine position
        let x = windowState.x;
        let y = windowState.y;

        // Check if the saved position is on-screen
        const onScreen = isPositionOnScreen(x, y, monitorData);

        if (!onScreen) {
          // Reposition to center of primary display
          const primaryData = primary
            ? { position: { x: primary.position.x, y: primary.position.y }, size: { width: primary.size.width, height: primary.size.height } }
            : null;
          const centered = getCenteredPosition(width, height, primaryData);
          x = centered.x;
          y = centered.y;
        }

        // Apply window size and position
        if (windowState.isMaximized) {
          await appWindow.maximize();
        } else {
          await appWindow.setSize(new PhysicalSize(width, height));
          await appWindow.setPosition(new PhysicalPosition(x, y));
        }
      } catch (err) {
        // If config loading fails, window stays at default tauri.conf.json settings
        console.warn("Failed to restore window state:", err);
      }
    }

    async function saveWindowState() {
      try {
        const appWindow = getCurrentWindow();
        const size = await appWindow.innerSize();
        const position = await appWindow.outerPosition();
        const isMaximized = await appWindow.isMaximized();

        // Store physical pixel values for window state
        const newWindowState: WindowState = {
          x: position.x,
          y: position.y,
          width: size.width,
          height: size.height,
          isMaximized,
        };

        // Persist through settingsStore so other config fields are never lost.
        useSettingsStore.getState().setWindowState(newWindowState);
      } catch (err) {
        console.warn("Failed to save window state:", err);
      }
    }

    function debouncedSave() {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
      debounceTimerRef.current = setTimeout(() => {
        saveWindowState();
      }, SAVE_DEBOUNCE_MS);
    }

    async function setupListeners() {
      // Listen for window resize events
      unlisten1 = await appWindow.onResized(() => {
        debouncedSave();
      });

      // Listen for window move events
      unlisten2 = await appWindow.onMoved(() => {
        debouncedSave();
      });
    }

    // Restore state on mount
    restoreWindowState();

    // Set up event listeners
    setupListeners();

    // Cleanup on unmount
    return () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
      }
      if (unlisten1) unlisten1();
      if (unlisten2) unlisten2();
    };
  }, []);
}
