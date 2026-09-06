import { create } from "zustand";

export type NavigationSection =
  | "home"
  | "askai"
  | "search"
  | "files"
  | "bookmarks"
  | "tasks"
  | "tools"
  | "settings";

interface NavigationState {
  activeSection: NavigationSection;
  /** Pending question/query handed from the home entry box to the AI or
   * search tab (home unmounts on section switch, so the value lives here). */
  pendingQuery: string;
  setActiveSection: (section: NavigationSection) => void;
  /** Stash a query and jump to the matching AI/search tab. */
  openWithQuery: (target: "askai" | "search", query: string) => void;
  /** Read and clear the pending query. */
  consumePendingQuery: () => string;
}

export const useNavigationStore = create<NavigationState>((set, get) => ({
  activeSection: "home",
  pendingQuery: "",
  setActiveSection: (section) => set({ activeSection: section }),
  openWithQuery: (target, query) =>
    set({ activeSection: target, pendingQuery: query }),
  consumePendingQuery: () => {
    const query = get().pendingQuery;
    set({ pendingQuery: "" });
    return query;
  },
}));
