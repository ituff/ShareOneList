import { useEffect } from "react";
import { useFileStore } from "../stores/fileStore";

/**
 * Global keyboard shortcuts for the file browser.
 * - Backspace: Navigate to parent folder (when focus is not in an input/textarea).
 *
 * This hook should be used in the FileBrowser component or at a higher level.
 */
export function useKeyboardShortcuts() {
  const navigateUp = useFileStore((s) => s.navigateUp);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const tagName = target.tagName;

      // Don't trigger shortcuts when user is typing in an input field
      if (tagName === "INPUT" || tagName === "TEXTAREA" || target.isContentEditable) {
        return;
      }

      if (e.key === "Backspace") {
        e.preventDefault();
        navigateUp();
      }
    };

    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [navigateUp]);
}
