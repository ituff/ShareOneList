import {
  Folder,
  File,
  Image,
  FileText,
  FileAudio,
  FileVideo,
  FileArchive,
  FileCode,
  FileSpreadsheet,
} from "lucide-react";
import type { DriveItem, LayoutMode } from "../../lib/types";
import { formatFileSize, formatDate } from "../../lib/formatters";

interface FileItemProps {
  item: DriveItem;
  layoutMode: LayoutMode;
  onNavigate: (folderId: string, folderName: string) => void;
  /** Whether selection mode is active. */
  selectionMode?: boolean;
  /** Whether this item is currently selected. */
  isSelected?: boolean;
  /** Called when the selection checkbox is toggled. */
  onToggleSelect?: (itemId: string) => void;
}

/** Return the appropriate icon component based on file type / mime. */
function getFileIcon(item: DriveItem) {
  if (item.isFolder) return Folder;

  const mime = item.mimeType?.toLowerCase() ?? "";
  const name = item.name.toLowerCase();

  if (mime.startsWith("image/") || /\.(png|jpe?g|gif|bmp|svg|webp|ico)$/.test(name)) {
    return Image;
  }
  if (mime.startsWith("video/") || /\.(mp4|mkv|avi|mov|wmv|webm)$/.test(name)) {
    return FileVideo;
  }
  if (mime.startsWith("audio/") || /\.(mp3|wav|flac|aac|ogg|wma)$/.test(name)) {
    return FileAudio;
  }
  if (/\.(zip|rar|7z|tar|gz|bz2)$/.test(name)) {
    return FileArchive;
  }
  if (/\.(xlsx?|csv|ods)$/.test(name)) {
    return FileSpreadsheet;
  }
  if (/\.(ts|tsx|js|jsx|py|rs|java|c|cpp|h|go|rb|php|html|css|json|yaml|yml|toml|xml)$/.test(name)) {
    return FileCode;
  }
  if (/\.(docx?|pdf|txt|md|rtf|odt|pptx?)$/.test(name) || mime.startsWith("text/")) {
    return FileText;
  }

  return File;
}

function getIconColor(item: DriveItem): string {
  if (item.isFolder) return "text-yellow-500";
  const name = item.name.toLowerCase();
  if (/\.(png|jpe?g|gif|bmp|svg|webp|ico)$/.test(name)) return "text-green-500";
  if (/\.(mp4|mkv|avi|mov|wmv|webm)$/.test(name)) return "text-purple-500";
  if (/\.(mp3|wav|flac|aac|ogg|wma)$/.test(name)) return "text-pink-500";
  return "text-blue-500";
}

function isImageFile(item: DriveItem): boolean {
  const mime = item.mimeType?.toLowerCase() ?? "";
  const name = item.name.toLowerCase();
  return mime.startsWith("image/") || /\.(png|jpe?g|gif|bmp|svg|webp|ico)$/.test(name);
}

/** Renders a single DriveItem in list view. */
function ListItem({ item, onNavigate, selectionMode, isSelected, onToggleSelect }: Omit<FileItemProps, "layoutMode">) {
  const Icon = getFileIcon(item);
  const iconColor = getIconColor(item);

  return (
    <div
      className={`flex items-center gap-3 rounded-md px-3 py-2 hover:bg-accent/50 transition-colors cursor-pointer select-none ${
        isSelected ? "bg-accent/40" : ""
      }`}
      onDoubleClick={() => {
        if (item.isFolder) onNavigate(item.id, item.name);
      }}
      role="row"
      aria-label={item.name}
    >
      {selectionMode && (
        <input
          type="checkbox"
          checked={isSelected ?? false}
          onChange={() => onToggleSelect?.(item.id)}
          onClick={(e) => e.stopPropagation()}
          className="h-4 w-4 rounded border-border accent-primary shrink-0"
          aria-label={`Select ${item.name}`}
        />
      )}
      <Icon className={`h-5 w-5 shrink-0 ${iconColor}`} />
      <span className="flex-1 truncate text-sm text-foreground" title={item.name}>
        {item.name}
      </span>
      <span className="w-24 text-right text-xs text-muted-foreground">
        {item.isFolder ? "—" : formatFileSize(item.size ?? 0)}
      </span>
      <span className="w-36 text-right text-xs text-muted-foreground">
        {formatDate(item.lastModified)}
      </span>
    </div>
  );
}

/** Renders a single DriveItem in grid view. */
function GridItem({ item, onNavigate, selectionMode, isSelected, onToggleSelect }: Omit<FileItemProps, "layoutMode">) {
  const Icon = getFileIcon(item);
  const iconColor = getIconColor(item);

  return (
    <div
      className={`flex flex-col items-center gap-2 rounded-lg border border-border bg-card p-4 hover:bg-accent/30 transition-colors cursor-pointer select-none relative ${
        isSelected ? "ring-2 ring-primary" : ""
      }`}
      onDoubleClick={() => {
        if (item.isFolder) onNavigate(item.id, item.name);
      }}
      role="gridcell"
      aria-label={item.name}
    >
      {selectionMode && (
        <input
          type="checkbox"
          checked={isSelected ?? false}
          onChange={() => onToggleSelect?.(item.id)}
          onClick={(e) => e.stopPropagation()}
          className="absolute top-2 left-2 h-4 w-4 rounded border-border accent-primary"
          aria-label={`Select ${item.name}`}
        />
      )}
      <Icon className={`h-10 w-10 ${iconColor}`} />
      <span className="w-full text-center text-xs text-foreground truncate" title={item.name}>
        {item.name}
      </span>
      <span className="text-xs text-muted-foreground">
        {item.isFolder ? "" : formatFileSize(item.size ?? 0)}
      </span>
    </div>
  );
}

/** Renders a single DriveItem in gallery view. */
function GalleryItem({ item, onNavigate, selectionMode, isSelected, onToggleSelect }: Omit<FileItemProps, "layoutMode">) {
  const Icon = getFileIcon(item);
  const iconColor = getIconColor(item);
  const showThumbnail = isImageFile(item);

  return (
    <div
      className={`flex flex-col rounded-lg border border-border bg-card overflow-hidden hover:bg-accent/30 transition-colors cursor-pointer select-none relative ${
        isSelected ? "ring-2 ring-primary" : ""
      }`}
      onDoubleClick={() => {
        if (item.isFolder) onNavigate(item.id, item.name);
      }}
      role="gridcell"
      aria-label={item.name}
    >
      {selectionMode && (
        <input
          type="checkbox"
          checked={isSelected ?? false}
          onChange={() => onToggleSelect?.(item.id)}
          onClick={(e) => e.stopPropagation()}
          className="absolute top-2 left-2 z-10 h-4 w-4 rounded border-border accent-primary"
          aria-label={`Select ${item.name}`}
        />
      )}
      <div className="flex h-32 items-center justify-center bg-muted/30">
        {showThumbnail && item.downloadUrl ? (
          <img
            src={item.downloadUrl}
            alt={item.name}
            className="h-full w-full object-cover"
            loading="lazy"
          />
        ) : (
          <Icon className={`h-12 w-12 ${iconColor}`} />
        )}
      </div>
      <div className="p-2">
        <span className="block text-xs text-foreground truncate" title={item.name}>
          {item.name}
        </span>
      </div>
    </div>
  );
}

/** Main FileItem component that delegates to the appropriate layout renderer. */
export function FileItem({ item, layoutMode, onNavigate, selectionMode, isSelected, onToggleSelect }: FileItemProps) {
  switch (layoutMode) {
    case "list":
      return <ListItem item={item} onNavigate={onNavigate} selectionMode={selectionMode} isSelected={isSelected} onToggleSelect={onToggleSelect} />;
    case "grid":
      return <GridItem item={item} onNavigate={onNavigate} selectionMode={selectionMode} isSelected={isSelected} onToggleSelect={onToggleSelect} />;
    case "gallery":
      return <GalleryItem item={item} onNavigate={onNavigate} selectionMode={selectionMode} isSelected={isSelected} onToggleSelect={onToggleSelect} />;
    default:
      return <ListItem item={item} onNavigate={onNavigate} selectionMode={selectionMode} isSelected={isSelected} onToggleSelect={onToggleSelect} />;
  }
}
