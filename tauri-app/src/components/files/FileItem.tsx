import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Folder,
  File,
  FileText,
  FileType2,
  FileImage,
  FileAudio,
  FileVideo,
  FileArchive,
  FileCode,
  FileSpreadsheet,
  Presentation,
  MoreHorizontal,
  Eye,
  Download,
  Pencil,
  Trash2,
  Share2,
  FileDown,
  Info,
  Copy,
  Link2,
  Bookmark,
} from "lucide-react";
import type { CloudEnvironment, DriveItem, LayoutMode } from "../../lib/types";
import { formatFileSize, formatDate } from "../../lib/formatters";
import { canConvertToPdf } from "./dialogs";
import { isPreviewable } from "./FilePreview";
import { getItemSize, getThumbnailUrl } from "../../lib/tauri";

export type FileItemAction =
  | "preview"
  | "download"
  | "rename"
  | "delete"
  | "share"
  | "convert"
  | "properties"
  | "copyName"
  | "copyLink"
  | "bookmark";

type FileKind =
  | "folder"
  | "image"
  | "video"
  | "audio"
  | "archive"
  | "pdf"
  | "word"
  | "excel"
  | "ppt"
  | "code"
  | "text"
  | "generic";

function getFileKind(item: DriveItem): FileKind {
  if (item.isFolder) return "folder";

  const mime = item.mimeType?.toLowerCase() ?? "";
  const name = item.name.toLowerCase();
  const ext = name.slice(name.lastIndexOf(".") + 1);

  if (mime.startsWith("image/") || /^(png|jpe?g|gif|bmp|svg|webp|ico|tiff|heic)$/.test(ext)) {
    return "image";
  }
  if (mime.startsWith("video/") || /^(mp4|mkv|avi|mov|wmv|webm|m4v|3gp|flv)$/.test(ext)) {
    return "video";
  }
  if (mime.startsWith("audio/") || /^(mp3|wav|flac|aac|ogg|wma|m4a|opus)$/.test(ext)) {
    return "audio";
  }
  if (
    mime.includes("zip") ||
    mime.includes("compressed") ||
    mime.includes("gzip") ||
    /^(zip|rar|7z|tar|gz|bz2|xz)$/.test(ext)
  ) {
    return "archive";
  }
  if (mime === "application/pdf" || ext === "pdf") return "pdf";
  if (
    mime.includes("wordprocessing") ||
    mime.includes("msword") ||
    /^(doc|docx|odt|rtf)$/.test(ext)
  ) {
    return "word";
  }
  if (
    mime.includes("spreadsheet") ||
    mime.includes("excel") ||
    mime.includes("csv") ||
    /^(xls|xlsx|ods|csv)$/.test(ext)
  ) {
    return "excel";
  }
  if (
    mime.includes("presentation") ||
    mime.includes("powerpoint") ||
    /^(ppt|pptx|odp)$/.test(ext)
  ) {
    return "ppt";
  }
  if (
    /^(ts|tsx|js|jsx|py|rs|java|c|cpp|h|hpp|cs|go|rb|php|html|css|scss|json|yaml|yml|toml|xml|sh|sql|kt|swift)$/.test(
      ext
    )
  ) {
    return "code";
  }
  if (mime.startsWith("text/") || /^(txt|md|markdown)$/.test(ext)) return "text";
  return "generic";
}

interface FileItemProps {
  item: DriveItem;
  layoutMode: LayoutMode;
  driveId: string;
  cloudEnv: CloudEnvironment;
  onNavigate: (folderId: string, folderName: string) => void;
  /** Whether selection mode is active. */
  selectionMode?: boolean;
  /** Whether this item is currently selected. */
  isSelected?: boolean;
  /** Called when the selection checkbox is toggled. */
  onToggleSelect?: (itemId: string) => void;
  /** Called when an item action is chosen. */
  onAction?: (item: DriveItem, action: FileItemAction) => void;
  /** Called when a file is double-clicked. */
  onOpen?: (item: DriveItem) => void;
}

/** Return the appropriate icon component based on file type / mime. */
function getFileIcon(item: DriveItem) {
  switch (getFileKind(item)) {
    case "folder":
      return Folder;
    case "image":
      return FileImage;
    case "video":
      return FileVideo;
    case "audio":
      return FileAudio;
    case "archive":
      return FileArchive;
    case "pdf":
      return FileText;
    case "word":
      return FileText;
    case "excel":
      return FileSpreadsheet;
    case "ppt":
      return Presentation;
    case "code":
      return FileCode;
    case "text":
      return FileType2;
    default:
      return File;
  }
}

function getIconColor(item: DriveItem): string {
  switch (getFileKind(item)) {
    case "folder":
      return "text-yellow-500";
    case "image":
      return "text-emerald-500";
    case "video":
      return "text-purple-500";
    case "audio":
      return "text-pink-500";
    case "archive":
      return "text-amber-600";
    case "pdf":
      return "text-red-500";
    case "word":
      return "text-blue-600";
    case "excel":
      return "text-green-600";
    case "ppt":
      return "text-orange-500";
    case "code":
      return "text-cyan-600";
    case "text":
      return "text-slate-500";
    default:
      return "text-blue-500";
  }
}

const thumbnailCache = new Map<string, string | null>();

interface ThumbnailProps {
  item: DriveItem;
  driveId: string;
  cloudEnv: CloudEnvironment;
  iconClassName?: string;
  imgClassName?: string;
}

function FileThumbnail({
  item,
  driveId,
  cloudEnv,
  iconClassName = "h-10 w-10",
  imgClassName,
}: ThumbnailProps) {
  const kind = getFileKind(item);
  const Icon = getFileIcon(item);
  const iconColor = getIconColor(item);
  const [thumbnailUrl, setThumbnailUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (kind !== "image" && kind !== "video") return;

    const key = `${cloudEnv}:${driveId}:${item.id}`;
    const cached = thumbnailCache.get(key);
    if (cached !== undefined) {
      setThumbnailUrl(cached);
      setFailed(cached === null);
      return;
    }

    let cancelled = false;
    getThumbnailUrl(driveId, item.id, cloudEnv)
      .then((url) => {
        if (cancelled) return;
        thumbnailCache.set(key, url);
        setThumbnailUrl(url);
        setFailed(false);
      })
      .catch(() => {
        if (cancelled) return;
        if (kind === "image" && item.downloadUrl) {
          thumbnailCache.set(key, item.downloadUrl);
          setThumbnailUrl(item.downloadUrl);
        } else {
          thumbnailCache.set(key, null);
          setThumbnailUrl(null);
          setFailed(true);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [item, driveId, cloudEnv, kind]);

  if (kind === "folder" || (kind !== "image" && kind !== "video") || !thumbnailUrl || failed) {
    return <Icon className={`shrink-0 ${iconColor} ${iconClassName}`} />;
  }

  return (
    <img
      src={thumbnailUrl}
      alt=""
      loading="lazy"
      className={imgClassName ?? iconClassName}
      onError={() => {
        thumbnailCache.set(`${cloudEnv}:${driveId}:${item.id}`, null);
        setThumbnailUrl(null);
        setFailed(true);
      }}
    />
  );
}

const sizeCache = new Map<string, number | null>();

function ItemSize({
  item,
  driveId,
  cloudEnv,
}: {
  item: DriveItem;
  driveId: string;
  cloudEnv: CloudEnvironment;
}) {
  const [size, setSize] = useState<number | null>(item.size ?? null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (item.size != null) {
      setSize(item.size);
      setLoading(false);
      return;
    }

    const key = `${cloudEnv}:${driveId}:${item.id}`;
    const cached = sizeCache.get(key);
    if (cached !== undefined) {
      setSize(cached);
      setLoading(false);
      return;
    }

    let cancelled = false;
    setLoading(true);
    getItemSize(driveId, item.id, cloudEnv)
      .then((value) => {
        if (cancelled) return;
        sizeCache.set(key, value);
        setSize(value);
      })
      .catch(() => {
        if (cancelled) return;
        sizeCache.set(key, null);
        setSize(null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [item, driveId, cloudEnv]);

  if (size != null) return <>{formatFileSize(size)}</>;
  if (loading) return <span className="text-muted-foreground">...</span>;
  return <span className="text-muted-foreground">—</span>;
}

/** Dropdown menu with file operations. */
function ActionMenu({ item, onAction }: { item: DriveItem; onAction: FileItemProps["onAction"] }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener("click", close);
    return () => window.removeEventListener("click", close);
  }, [open]);

  const actions = [
    {
      key: "preview",
      label: t("preview.title"),
      icon: <Eye className="h-3.5 w-3.5" />,
      show: !item.isFolder && isPreviewable(item),
    },
    {
      key: "download",
      label: t("fileOps.download"),
      icon: <Download className="h-3.5 w-3.5" />,
      show: true,
    },
    {
      key: "copyName",
      label: t("fileOps.copyFilename"),
      icon: <Copy className="h-3.5 w-3.5" />,
      show: true,
    },
    {
      key: "copyLink",
      label: t("fileOps.copyDownloadUrl"),
      icon: <Link2 className="h-3.5 w-3.5" />,
      show: !item.isFolder && Boolean(item.downloadUrl || item.webUrl),
    },
    {
      key: "bookmark",
      label: t("fileOps.bookmark"),
      icon: <Bookmark className="h-3.5 w-3.5" />,
      show: true,
    },
    {
      key: "rename",
      label: t("fileOps.rename"),
      icon: <Pencil className="h-3.5 w-3.5" />,
      show: true,
    },
    {
      key: "delete",
      label: t("fileOps.delete"),
      icon: <Trash2 className="h-3.5 w-3.5" />,
      show: true,
    },
    {
      key: "share",
      label: t("fileOps.share"),
      icon: <Share2 className="h-3.5 w-3.5" />,
      show: true,
    },
    {
      key: "convert",
      label: t("fileOps.convert"),
      icon: <FileDown className="h-3.5 w-3.5" />,
      show: !item.isFolder && canConvertToPdf(item.name),
    },
    {
      key: "properties",
      label: t("fileOps.properties"),
      icon: <Info className="h-3.5 w-3.5" />,
      show: true,
    },
  ] as const;
  const visibleActions = actions.filter((action) => action.show);

  return (
    <div className="relative shrink-0">
      <button
        onClick={(e) => {
          e.stopPropagation();
          setOpen((value) => !value);
        }}
        className="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
        aria-label={t("fileOps.open")}
        title={t("fileOps.open")}
      >
        <MoreHorizontal className="h-4 w-4" />
      </button>
      {open && (
        <div className="absolute right-0 top-full z-30 mt-1 min-w-[150px] rounded-md border border-border bg-card p-1 shadow-lg">
          {visibleActions.map((action) => (
            <button
              key={action.key}
              onClick={(e) => {
                e.stopPropagation();
                setOpen(false);
                onAction?.(item, action.key);
              }}
              className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm text-foreground hover:bg-accent transition-colors"
            >
              {action.icon}
              <span>{action.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
/** Renders a single DriveItem in list view. */
function ListItem({ item, driveId, cloudEnv, onNavigate, selectionMode, isSelected, onToggleSelect, onAction, onOpen }: Omit<FileItemProps, "layoutMode">) {
  return (
    <div
      className={`flex items-center gap-3 rounded-md px-3 py-2 hover:bg-accent/50 transition-colors cursor-pointer select-none ${
        isSelected ? "bg-accent/40" : ""
      }`}
      onClick={() => {
        if (!selectionMode && item.isFolder) {
          onNavigate(item.id, item.name);
        }
      }}
      onDoubleClick={() => {
        if (item.isFolder) onNavigate(item.id, item.name);
        else onOpen?.(item);
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
      <FileThumbnail
        item={item}
        driveId={driveId}
        cloudEnv={cloudEnv}
        iconClassName="h-5 w-5"
        imgClassName="h-5 w-5 shrink-0 rounded object-cover"
      />
      <span className="flex-1 truncate text-sm text-foreground" title={item.name}>
        {item.name}
      </span>
      <span className="w-24 text-right text-xs text-muted-foreground">
        <ItemSize item={item} driveId={driveId} cloudEnv={cloudEnv} />
      </span>
      <span className="w-36 text-right text-xs text-muted-foreground">
        {formatDate(item.lastModified)}
      </span>
      <ActionMenu item={item} onAction={onAction} />
    </div>
  );
}

/** Renders a single DriveItem in grid view. */
function GridItem({ item, driveId, cloudEnv, onNavigate, selectionMode, isSelected, onToggleSelect, onAction, onOpen }: Omit<FileItemProps, "layoutMode">) {
  return (
    <div
      className={`flex flex-col items-center gap-2 rounded-lg border border-border bg-card p-4 hover:bg-accent/30 transition-colors cursor-pointer select-none relative ${
        isSelected ? "ring-2 ring-primary" : ""
      }`}
      onClick={() => {
        if (!selectionMode && item.isFolder) {
          onNavigate(item.id, item.name);
        }
      }}
      onDoubleClick={() => {
        if (item.isFolder) onNavigate(item.id, item.name);
        else onOpen?.(item);
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
      <FileThumbnail
        item={item}
        driveId={driveId}
        cloudEnv={cloudEnv}
        iconClassName="h-10 w-10"
        imgClassName="h-10 w-10 rounded object-cover"
      />
      <span className="w-full text-center text-xs text-foreground truncate" title={item.name}>
        {item.name}
      </span>
      <span className="text-xs text-muted-foreground">
        <ItemSize item={item} driveId={driveId} cloudEnv={cloudEnv} />
      </span>
      <ActionMenu item={item} onAction={onAction} />
    </div>
  );
}

/** Renders a single DriveItem in gallery view. */
function GalleryItem({ item, driveId, cloudEnv, onNavigate, selectionMode, isSelected, onToggleSelect, onAction, onOpen }: Omit<FileItemProps, "layoutMode">) {
  return (
    <div
      className={`flex flex-col rounded-lg border border-border bg-card overflow-hidden hover:bg-accent/30 transition-colors cursor-pointer select-none relative ${
        isSelected ? "ring-2 ring-primary" : ""
      }`}
      onClick={() => {
        if (!selectionMode && item.isFolder) {
          onNavigate(item.id, item.name);
        }
      }}
      onDoubleClick={() => {
        if (item.isFolder) onNavigate(item.id, item.name);
        else onOpen?.(item);
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
        <FileThumbnail
          item={item}
          driveId={driveId}
          cloudEnv={cloudEnv}
          iconClassName="h-12 w-12"
          imgClassName="h-full w-full object-cover"
        />
      </div>
      <div className="p-2">
        <span className="block text-xs text-foreground truncate" title={item.name}>
          {item.name}
        </span>
        <span className="block text-xs text-muted-foreground mt-0.5">
          <ItemSize item={item} driveId={driveId} cloudEnv={cloudEnv} />
        </span>
      </div>
      <ActionMenu item={item} onAction={onAction} />
    </div>
  );
}

/** Main FileItem component that delegates to the appropriate layout renderer. */
export function FileItem({ item, layoutMode, driveId, cloudEnv, onNavigate, selectionMode, isSelected, onToggleSelect, onAction, onOpen }: FileItemProps) {
  switch (layoutMode) {
    case "list":
      return <ListItem item={item} driveId={driveId} cloudEnv={cloudEnv} onNavigate={onNavigate} selectionMode={selectionMode} isSelected={isSelected} onToggleSelect={onToggleSelect} onAction={onAction} onOpen={onOpen} />;
    case "grid":
      return <GridItem item={item} driveId={driveId} cloudEnv={cloudEnv} onNavigate={onNavigate} selectionMode={selectionMode} isSelected={isSelected} onToggleSelect={onToggleSelect} onAction={onAction} onOpen={onOpen} />;
    case "gallery":
      return <GalleryItem item={item} driveId={driveId} cloudEnv={cloudEnv} onNavigate={onNavigate} selectionMode={selectionMode} isSelected={isSelected} onToggleSelect={onToggleSelect} onAction={onAction} onOpen={onOpen} />;
    default:
      return <ListItem item={item} driveId={driveId} cloudEnv={cloudEnv} onNavigate={onNavigate} selectionMode={selectionMode} isSelected={isSelected} onToggleSelect={onToggleSelect} onAction={onAction} onOpen={onOpen} />;
  }
}
