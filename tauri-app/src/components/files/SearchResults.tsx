import { useTranslation } from "react-i18next";
import { Loader2, SearchX, File, Folder } from "lucide-react";
import type { DriveItem } from "../../lib/types";
import { formatFileSize, formatDate } from "../../lib/formatters";

interface SearchResultsProps {
  results: DriveItem[];
  isSearching: boolean;
  onNavigateToParent: (item: DriveItem) => void;
}

/** Extract display path from parentReference.path (remove drive prefix). */
function getParentPath(item: DriveItem): string {
  const raw = item.parentReference?.path;
  if (!raw) return "";
  // Path format: /drive/root:/folder/subfolder — strip prefix up to the colon
  const colonIdx = raw.indexOf(":");
  if (colonIdx >= 0) {
    const path = raw.slice(colonIdx + 1);
    return path || "/";
  }
  return raw;
}

export function SearchResults({ results, isSearching, onNavigateToParent }: SearchResultsProps) {
  const { t } = useTranslation();

  // Loading state
  if (isSearching) {
    return (
      <div className="flex items-center justify-center py-16">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  // Empty state
  if (results.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center gap-3 py-16">
        <SearchX className="h-10 w-10 text-muted-foreground/50" />
        <p className="text-sm text-muted-foreground">{t("search.noResults")}</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col">
      {/* Header */}
      <div className="flex items-center gap-3 px-3 py-1 border-b border-border text-xs text-muted-foreground font-medium">
        <span className="w-5" />
        <span className="flex-1">{t("fileBrowser.name")}</span>
        <span className="w-40 text-left">{t("properties.filename")}</span>
        <span className="w-24 text-right">{t("fileBrowser.size")}</span>
        <span className="w-36 text-right">{t("fileBrowser.modified")}</span>
      </div>

      {/* Results list */}
      {results.map((item) => (
        <div
          key={item.id}
          className="flex items-center gap-3 rounded-md px-3 py-2 hover:bg-accent/50 transition-colors cursor-pointer select-none"
          onDoubleClick={() => onNavigateToParent(item)}
          role="row"
          aria-label={item.name}
          title={t("search.modeLocalDesc")}
        >
          {item.isFolder ? (
            <Folder className="h-5 w-5 shrink-0 text-yellow-500" />
          ) : (
            <File className="h-5 w-5 shrink-0 text-blue-500" />
          )}
          <span className="flex-1 truncate text-sm text-foreground" title={item.name}>
            {item.name}
          </span>
          <span className="w-40 truncate text-left text-xs text-muted-foreground" title={getParentPath(item)}>
            {getParentPath(item)}
          </span>
          <span className="w-24 text-right text-xs text-muted-foreground">
            {item.isFolder ? "—" : formatFileSize(item.size ?? 0)}
          </span>
          <span className="w-36 text-right text-xs text-muted-foreground">
            {formatDate(item.lastModified)}
          </span>
        </div>
      ))}
    </div>
  );
}
