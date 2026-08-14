import { useTranslation } from "react-i18next";
import { Bookmark, Folder, FileText, Trash2 } from "lucide-react";
import { useBookmarkStore } from "../../stores/bookmarkStore";
import { useTabStore } from "../../stores/tabStore";
import type { BookmarkEntry, DriveItem } from "../../lib/types";

function toDriveItem(bookmark: BookmarkEntry): DriveItem {
  return {
    id: bookmark.itemId,
    name: bookmark.name,
    size: null,
    lastModified: bookmark.createdAt,
    isFolder: bookmark.isFolder,
    mimeType: null,
    webUrl: null,
    parentReference: null,
    downloadUrl: null,
    createdDateTime: bookmark.createdAt,
  };
}

export function BookmarksPage() {
  const { t } = useTranslation();
  const bookmarks = useBookmarkStore((s) => s.bookmarks);
  const removeBookmark = useBookmarkStore((s) => s.removeBookmark);

  const handleOpen = (bookmark: BookmarkEntry) => {
    if (bookmark.isFolder) {
      useTabStore.getState().openTabAtFolder(
        bookmark.driveId,
        bookmark.driveName,
        bookmark.cloudEnv,
        bookmark.homeAccountId,
        bookmark.itemId,
        bookmark.name
      );
    } else {
      useTabStore.getState().openPreviewTab(
        toDriveItem(bookmark),
        bookmark.driveId,
        bookmark.cloudEnv,
        bookmark.homeAccountId
      );
    }
  };

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold text-foreground">{t("nav.bookmarks")}</h2>
        <p className="text-muted-foreground">{t("bookmarks.description")}</p>
      </div>

      {bookmarks.length === 0 ? (
        <div className="rounded-lg border border-dashed border-border p-10 text-center space-y-2">
          <Bookmark className="h-10 w-10 text-muted-foreground/50 mx-auto" />
          <p className="text-sm text-muted-foreground">{t("bookmarks.empty")}</p>
        </div>
      ) : (
        <div className="space-y-2">
          {bookmarks.map((bookmark) => (
            <div
              key={bookmark.id}
              className="flex items-center gap-3 rounded-lg border border-border bg-card px-4 py-3 hover:bg-accent/30 transition-colors cursor-pointer"
              onClick={() => handleOpen(bookmark)}
            >
              {bookmark.isFolder ? (
                <Folder className="h-5 w-5 text-yellow-500 shrink-0" />
              ) : (
                <FileText className="h-5 w-5 text-blue-500 shrink-0" />
              )}
              <div className="min-w-0 flex-1">
                <div className="text-sm font-medium text-foreground truncate">
                  {bookmark.name}
                </div>
                <div className="text-xs text-muted-foreground truncate">
                  {bookmark.driveName} · {bookmark.cloudEnv === "global" ? t("accounts.cloudGlobal") : t("accounts.cloudChina")}
                </div>
              </div>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  removeBookmark(bookmark.id);
                }}
                className="rounded-md p-1.5 text-muted-foreground hover:bg-destructive/10 hover:text-destructive transition-colors"
                title={t("bookmarks.remove")}
              >
                <Trash2 className="h-4 w-4" />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
