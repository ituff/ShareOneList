import { create } from "zustand";

/** Kinds of notifications shown in the notification center. */
export type NotificationKind = "update" | "download" | "download-error";

export interface AppNotification {
  id: string;
  kind: NotificationKind;
  /** Primary line, composed at push time. */
  title: string;
  /** Optional detail line (file name, error message, release notes…). */
  detail?: string;
  createdAt: number;
  read: boolean;
}

/** Maximum notifications kept in memory. */
const MAX_NOTIFICATIONS = 50;

interface NotificationState {
  notifications: AppNotification[];
  /** Whether the dropdown panel is open. */
  panelOpen: boolean;

  push: (notification: Omit<AppNotification, "id" | "createdAt" | "read">) => void;
  markAllRead: () => void;
  clearAll: () => void;
  setPanelOpen: (open: boolean) => void;
}

let nextId = 1;

export const useNotificationStore = create<NotificationState>((set) => ({
  notifications: [],
  panelOpen: false,

  push: (notification) =>
    set((state) => ({
      panelOpen: state.panelOpen,
      notifications: [
        { ...notification, id: `n-${nextId++}`, createdAt: Date.now(), read: false },
        ...state.notifications,
      ].slice(0, MAX_NOTIFICATIONS),
    })),

  markAllRead: () =>
    set((state) => ({
      notifications: state.notifications.map((n) => ({ ...n, read: true })),
    })),

  clearAll: () => set({ notifications: [] }),

  setPanelOpen: (open) =>
    set((state) => ({
      panelOpen: open,
      // Opening the panel marks everything as read.
      notifications: open
        ? state.notifications.map((n) => ({ ...n, read: true }))
        : state.notifications,
    })),
}));
