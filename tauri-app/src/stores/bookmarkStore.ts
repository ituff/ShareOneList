import { create } from "zustand";
import type { BookmarkEntry, CloudEnvironment, DriveItem } from "../lib/types";

const STORAGE_KEY = "shareonelist.bookmarks";

interface BookmarkStoreState {
  bookmarks: BookmarkEntry[];
  addBookmark: (input: {
    item: DriveItem;
    driveId: string;
    driveName: string;
    cloudEnv: CloudEnvironment;
    homeAccountId: string;
  }) => void;
  removeBookmark: (bookmarkId: string) => void;
}

function loadBookmarks(): BookmarkEntry[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as BookmarkEntry[];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

export const useBookmarkStore = create<BookmarkStoreState>((set) => ({
  bookmarks: loadBookmarks(),

  addBookmark: ({ item, driveId, driveName, cloudEnv, homeAccountId }) => {
    const id = `${homeAccountId}:${driveId}:${item.id}`;
    const existing = loadBookmarks().some((bookmark) => bookmark.id === id);
    if (existing) return;

    const entry: BookmarkEntry = {
      id,
      name: item.name,
      driveId,
      driveName,
      itemId: item.id,
      cloudEnv,
      homeAccountId,
      isFolder: item.isFolder,
      createdAt: new Date().toISOString(),
    };

    const bookmarks = [...loadBookmarks(), entry];
    localStorage.setItem(STORAGE_KEY, JSON.stringify(bookmarks));
    set({ bookmarks });
  },

  removeBookmark: (bookmarkId) => {
    const bookmarks = loadBookmarks().filter((bookmark) => bookmark.id !== bookmarkId);
    localStorage.setItem(STORAGE_KEY, JSON.stringify(bookmarks));
    set({ bookmarks });
  },
}));
