/**
 * Hook for managing the application theme (light/dark/system).
 *
 * Responsibilities:
 *   - Reads the current theme preference from settingsStore
 *   - Applies or removes the "dark" class on document.documentElement
 *   - When theme is "system", detects OS preference via matchMedia
 *   - Listens for OS theme changes and reacts in real-time (system mode)
 *   - Reacts to store theme changes and updates the DOM class accordingly
 *
 * The hook does NOT persist the theme — that's handled by settingsStore.setTheme().
 */

import { useEffect } from "react";
import { useSettingsStore } from "../stores/settingsStore";

/** Media query for detecting OS dark mode preference. */
const DARK_MODE_QUERY = "(prefers-color-scheme: dark)";

/**
 * Resolves whether dark mode should be active based on the theme setting.
 */
function shouldUseDarkMode(theme: "light" | "dark" | "system"): boolean {
  if (theme === "dark") return true;
  if (theme === "light") return false;
  // "system" — check OS preference
  return window.matchMedia(DARK_MODE_QUERY).matches;
}

/**
 * Applies or removes the "dark" class on the root <html> element.
 */
function applyDarkClass(isDark: boolean): void {
  const root = document.documentElement;
  if (isDark) {
    root.classList.add("dark");
  } else {
    root.classList.remove("dark");
  }
}

/**
 * useTheme — call once in App.tsx to keep the document's dark mode class
 * synchronized with the user's theme preference and the OS setting.
 */
export function useTheme(): void {
  const theme = useSettingsStore((state) => state.theme);

  useEffect(() => {
    // Apply the correct class immediately based on current theme
    applyDarkClass(shouldUseDarkMode(theme));

    // If not in "system" mode, no need to listen for OS changes
    if (theme !== "system") {
      return;
    }

    // Listen for OS theme changes while in system mode
    const mediaQuery = window.matchMedia(DARK_MODE_QUERY);

    const handleChange = (event: MediaQueryListEvent) => {
      applyDarkClass(event.matches);
    };

    mediaQuery.addEventListener("change", handleChange);

    return () => {
      mediaQuery.removeEventListener("change", handleChange);
    };
  }, [theme]);
}
