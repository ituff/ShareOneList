import { create } from "zustand";

export type NavigationSection =
  | "home"
  | "files"
  | "bookmarks"
  | "tasks"
  | "tools"
  | "settings";

interface NavigationState {
  activeSection: NavigationSection;
  setActiveSection: (section: NavigationSection) => void;
}

export const useNavigationStore = create<NavigationState>((set) => ({
  activeSection: "home",
  setActiveSection: (section) => set({ activeSection: section }),
}));
