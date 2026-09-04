import { useEffect, useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { dirname, downloadDir, join } from "@tauri-apps/api/path";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  List,
  LayoutGrid,
  GalleryHorizontalEnd,
  Loader2,
  FolderOpen,
  AlertCircle,
  ArrowLeft,
  ArrowDown,
  ArrowUp,
  CheckSquare,
  Download,
  Upload,
  FolderPlus,
  RefreshCw,
} from "lucide-react";
import { useTabStore } from "../../stores/tabStore";
import { useTaskStore } from "../../stores/taskStore";
import { useToastStore } from "../../stores/toastStore";
import { useSettingsStore } from "../../stores/settingsStore";
import { useBookmarkStore } from "../../stores/bookmarkStore";
import { FileItem, type FileItemAction } from "./FileItem";
import { SearchBar } from "./SearchBar";
import { SearchResults } from "./SearchResults";
import { FolderBreadcrumb } from "./FolderBreadcrumb";
import { isPreviewable } from "./FilePreview";
import {
  ConvertDialog,
  CreateFolderDialog,
  DeleteDialog,
  PropertiesDialog,
  RenameDialog,
  ShareLinkDialog,
} from "./dialogs";
import {
  searchFiles,
  uploadFiles,
  downloadFile,
  downloadFiles,
  downloadFolder,
} from "../../lib/tauri";
import type {
  CloudEnvironment,
  DownloadFileSpec,
  DriveItem,
  LayoutMode,
  SearchScope,
  SortKey,
} from "../../lib/types";
import { getErrorMessage } from "../../lib/errors";

interface FileBrowserProps {
  tabId: string;
  /** Whether this tab is the visible one; gates window-level listeners. */
  isActive: boolean;
  driveId: string;
  homeAccountId: string;
  cloudEnv: CloudEnvironment;
  driveName: string;
  /** Open a file in a new preview tab. */
  onOpenPreview: (item: DriveItem) => void;
  /** Leave for the account service page when Back is pressed at the drive root. */
  onExitToHub: () => void;
}

type ActionDialog =
  | { kind: "createFolder" }
  | { kind: "rename" | "delete" | "share" | "convert" | "properties"; item: DriveItem };

/** Toolbar button for layout mode toggle. */
function LayoutButton({
  mode,
  activeMode,
  icon: Icon,
  label,
  onClick,
}: {
  mode: LayoutMode;
  activeMode: LayoutMode;
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  onClick: (mode: LayoutMode) => void;
}) {
  const isActive = mode === activeMode;
  return (
    <button
      onClick={() => onClick(mode)}
      className={`rounded-md p-1.5 transition-colors ${
        isActive
          ? "bg-primary text-primary-foreground"
          : "text-muted-foreground hover:bg-accent"
      }`}
      aria-label={label}
      title={label}
    >
      <Icon className="h-4 w-4" />
    </button>
  );
}

/** Clickable column header: click toggles ascending/descending for that column. */
function SortHeaderButton({
  label,
  column,
  activeKey,
  asc,
  className,
  tabId,
  onChange,
}: {
  label: string;
  column: SortKey;
  activeKey: SortKey;
  asc: boolean;
  className?: string;
  tabId: string;
  onChange: (tabId: string, key: SortKey) => void;
}) {
  const { t } = useTranslation();
  const isActive = column === activeKey;
  const Arrow = asc ? ArrowUp : ArrowDown;
  // Title shows what the NEXT click does.
  const nextDirection = asc
    ? t("fileBrowser.sortDescending")
    : t("fileBrowser.sortAscending");
  return (
    <button
      onClick={() => onChange(tabId, column)}
      aria-sort={isActive ? (asc ? "ascending" : "descending") : "none"}
      title={isActive ? `${label} → ${nextDirection}` : label}
      className={`flex items-center gap-1 hover:text-foreground transition-colors ${
        isActive ? "text-foreground" : ""
      } ${className ?? ""}`}
    >
      <span>{label}</span>
      {isActive && <Arrow className="h-3 w-3" />}
    </button>
  );
}

export function FileBrowser({ tabId, isActive, driveId, homeAccountId, cloudEnv, driveName, onOpenPreview, onExitToHub }: FileBrowserProps) {
  const { t } = useTranslation();
  const loadFolder = useTabStore((s) => s.loadFolder);
  const navigateToFolder = useTabStore((s) => s.navigateToFolder);
  const navigateToBreadcrumb = useTabStore((s) => s.navigateToBreadcrumb);
  const navigateUp = useTabStore((s) => s.navigateUp);
  const navigateToRoot = useTabStore((s) => s.navigateToRoot);
  const setTabLayoutMode = useTabStore((s) => s.setTabLayoutMode);
  const setTabSort = useTabStore((s) => s.setTabSort);
  const tab = useTabStore((s) => s.tabs.find((t) => t.id === tabId));
  const addToast = useToastStore((s) => s.addToast);
  const lastDownloadPath = useSettingsStore((s) => s.lastDownloadPath);
  const setLastDownloadPath = useSettingsStore((s) => s.setLastDownloadPath);

  const items = tab?.items ?? [];
  const isLoading = tab?.isLoading ?? false;
  const error = tab?.error ?? null;
  const breadcrumbs = tab?.breadcrumbs ?? [];
  const layoutMode = tab?.layoutMode ?? "list";
  const currentFolderId = tab?.currentFolderId ?? null;
  const sortKey = tab?.sortKey ?? "name";
  const sortAsc = tab?.sortAsc ?? true;

  // Search state (local to FileBrowser)
  const [searchQuery, setSearchQuery] = useState("");
  const [searchScope, setSearchScope] = useState<SearchScope>("global");
  const [searchResults, setSearchResults] = useState<DriveItem[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Drag-and-drop state
  const [isDragOver, setIsDragOver] = useState(false);

  // Multi-select state
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedItems, setSelectedItems] = useState<Set<string>>(new Set());
  const [dialog, setDialog] = useState<ActionDialog | null>(null);

  // Load root folder on mount
  useEffect(() => {
    loadFolder(tabId, "root");
  }, [tabId, loadFolder]);

  // Clear selection when navigating to a different folder
  useEffect(() => {
    setSelectedItems(new Set());
  }, [currentFolderId]);

  // Listen for Tauri drag-drop events (OS file drop)
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;

    const setup = async () => {
      unlisten = await listen<{ paths: string[] }>("tauri://drag-drop", (event) => {
        const paths = event.payload.paths;
        if (paths && paths.length > 0 && currentFolderId) {
          handleFileDrop(paths);
        }
        setIsDragOver(false);
      });
    };

    setup();

    return () => {
      unlisten?.();
    };
  }, [tabId, currentFolderId]);

  // Listen for drag-enter/drag-over from Tauri (to show overlay)
  useEffect(() => {
    let unlistenEnter: UnlistenFn | undefined;
    let unlistenLeave: UnlistenFn | undefined;

    const setup = async () => {
      unlistenEnter = await listen("tauri://drag-enter", () => {
        setIsDragOver(true);
      });
      unlistenLeave = await listen("tauri://drag-leave", () => {
        setIsDragOver(false);
      });
    };

    setup();

    return () => {
      unlistenEnter?.();
      unlistenLeave?.();
    };
  }, []);

  const reloadFolder = useCallback(() => {
    if (currentFolderId) {
      loadFolder(tabId, currentFolderId);
    }
  }, [tabId, currentFolderId, loadFolder]);

  /** Handle file drop: upload dropped files to current folder. */
  const handleFileDrop = useCallback(
    async (filePaths: string[]) => {
      // Hidden (background) tabs must not react to drops meant for the visible one.
      if (!currentFolderId || !isActive) return;
      try {
        const taskIds = await uploadFiles(driveId, currentFolderId, filePaths, cloudEnv);
        taskIds.forEach((taskId, index) => {
          const filePath = filePaths[index] ?? "";
          const fileName = filePath.split(/[\\/]/).pop() ?? filePath;
          useTaskStore.getState().registerTask(taskId, {
            type: "upload",
            fileName,
            driveId,
            cloudEnv,
            localPath: filePath,
          });
        });
        reloadFolder();
      } catch (err) {
        addToast("error", getErrorMessage(err));
      }
    },
    [driveId, currentFolderId, isActive, cloudEnv, loadFolder, reloadFolder, addToast]
  );

  // HTML5 drag handlers for visual feedback (complements Tauri events)
  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(false);
  }, []);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(false);
    // Actual file paths come from tauri://drag-drop event, not from DataTransfer
  }, []);

  // Debounced search effect
  useEffect(() => {
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
    }

    if (!searchQuery.trim()) {
      setSearchResults([]);
      setIsSearching(false);
      return;
    }

    setIsSearching(true);
    debounceRef.current = setTimeout(async () => {
      try {
        const results = await searchFiles(
          driveId,
          searchQuery.trim(),
          searchScope,
          cloudEnv,
          searchScope === "local" ? (currentFolderId ?? undefined) : undefined
        );
        setSearchResults(results);
      } catch {
        setSearchResults([]);
      } finally {
        setIsSearching(false);
      }
    }, 300);

    return () => {
      if (debounceRef.current) {
        clearTimeout(debounceRef.current);
      }
    };
  }, [searchQuery, searchScope, driveId, currentFolderId, cloudEnv]);

  const handleClearSearch = useCallback(() => {
    setSearchQuery("");
    setSearchResults([]);
    setIsSearching(false);
  }, []);

  /** Navigate to the parent folder of a search result item. */
  const handleNavigateToParent = useCallback(
    (item: DriveItem) => {
      const parentId = item.parentReference?.id;
      const parentName = item.parentReference?.name;
      if (parentId) {
        handleClearSearch();
        navigateToFolder(tabId, parentId, parentName ?? "Folder");
      }
    },
    [tabId, navigateToFolder, handleClearSearch]
  );

  const handleRetry = () => {
    if (currentFolderId) {
      loadFolder(tabId, currentFolderId);
    }
  };

  /** Navigate to root when clicking the home breadcrumb. */
  const handleNavigateRoot = () => {
    navigateToRoot(tabId);
  };

  /** Toggle selection mode on/off. */
  const toggleSelectionMode = useCallback(() => {
    setSelectionMode((prev) => {
      if (prev) {
        // Exiting selection mode clears selection
        setSelectedItems(new Set());
      }
      return !prev;
    });
  }, []);

  /** Toggle selection for a single item. */
  const handleToggleSelect = useCallback((itemId: string) => {
    setSelectedItems((prev) => {
      const next = new Set(prev);
      if (next.has(itemId)) {
        next.delete(itemId);
      } else {
        next.add(itemId);
      }
      return next;
    });
  }, []);

  const handleDownloadItem = useCallback(
    async (item: DriveItem) => {
      try {
        const dir = lastDownloadPath ?? (await downloadDir());
        let localPath: string;

        if (item.isFolder) {
          const selectedDir = await open({
            directory: true,
            defaultPath: dir,
          });
          if (!selectedDir || Array.isArray(selectedDir)) return;
          setLastDownloadPath(selectedDir);
          localPath = await join(selectedDir, item.name);
        } else {
          const defaultPath = await join(dir, item.name);
          const selected = await save({ defaultPath });
          if (!selected) return;
          setLastDownloadPath(await dirname(selected));
          localPath = selected;
        }

        if (item.isFolder) {
          const batch = await downloadFolder(
            driveId,
            item.id,
            localPath,
            cloudEnv,
            homeAccountId,
            item.name
          );
          useTaskStore.getState().registerTask(batch.batchId, {
            type: "download",
            fileName: batch.batchName,
            homeAccountId,
            driveId,
            cloudEnv,
            itemId: item.id,
            localPath,
          });
        } else {
          const batch = await downloadFile(
            driveId,
            item.id,
            homeAccountId,
            item.name,
            item.size ?? 0,
            localPath,
            cloudEnv
          );
          useTaskStore.getState().registerTask(batch.batchId, {
            type: "download",
            fileName: batch.batchName,
            homeAccountId,
            driveId,
            cloudEnv,
            itemId: item.id,
            localPath,
          });
        }
      } catch (err) {
        addToast("error", getErrorMessage(err));
      }
    },
    [driveId, cloudEnv, addToast, lastDownloadPath, setLastDownloadPath]
  );

  const handleFileAction = useCallback(
    (item: DriveItem, action: FileItemAction) => {
      if (action === "download") {
        handleDownloadItem(item);
        return;
      }
      if (action === "preview") {
        onOpenPreview(item);
        return;
      }
      if (action === "copyName") {
        navigator.clipboard
          .writeText(item.name)
          .then(() => addToast("success", t("success.copied")))
          .catch((err) => addToast("error", getErrorMessage(err)));
        return;
      }
      if (action === "copyLink") {
        const url = item.downloadUrl ?? item.webUrl;
        if (!url) return;
        navigator.clipboard
          .writeText(url)
          .then(() => addToast("success", t("success.copied")))
          .catch((err) => addToast("error", getErrorMessage(err)));
        return;
      }
      if (action === "bookmark") {
        useBookmarkStore.getState().addBookmark({
          item,
          driveId,
          driveName,
          cloudEnv,
          homeAccountId,
        });
        addToast("success", t("success.bookmarked"));
        return;
      }
      setDialog({ kind: action, item });
    },
    [handleDownloadItem, onOpenPreview, addToast, t, driveId, driveName, cloudEnv, homeAccountId]
  );

  const handleFileOpen = useCallback(
    async (item: DriveItem) => {
      if (item.isFolder) {
        navigateToFolder(tabId, item.id, item.name);
        return;
      }
      if (isPreviewable(item)) {
        onOpenPreview(item);
      } else {
        await handleDownloadItem(item);
      }
    },
    [tabId, navigateToFolder, onOpenPreview, handleDownloadItem]
  );

  /** Download all selected items. */
  const handleDownloadSelected = useCallback(async () => {
    const selected = items.filter((item) => selectedItems.has(item.id));
    const files = selected.filter((item) => !item.isFolder);
    const folders = selected.filter((item) => item.isFolder);

    if (files.length > 0) {
      const dir = lastDownloadPath ?? (await downloadDir());
      const selectedDir = await open({
        directory: true,
        defaultPath: dir,
      });
      if (!selectedDir || Array.isArray(selectedDir)) return;
      setLastDownloadPath(selectedDir);

      const specs: DownloadFileSpec[] = files.map((file) => ({
        itemId: file.id,
        fileName: file.name,
        fileSize: file.size ?? 0,
      }));
      const batchName = t("tasks.batchDownload", { count: files.length });
      const batch = await downloadFiles(
        driveId,
        homeAccountId,
        specs,
        selectedDir,
        cloudEnv,
        batchName
      );
      useTaskStore.getState().registerTask(batch.batchId, {
        type: "download",
        fileName: batchName,
        homeAccountId,
        driveId,
        cloudEnv,
        localPath: selectedDir,
      });
    }

    for (const folder of folders) {
      await handleDownloadItem(folder);
    }

    // Clear selection after initiating downloads
    setSelectedItems(new Set());
    setSelectionMode(false);
  }, [
    items,
    selectedItems,
    handleDownloadItem,
    driveId,
    homeAccountId,
    cloudEnv,
    lastDownloadPath,
    setLastDownloadPath,
    t,
  ]);

  // Keyboard shortcuts for common file browser actions.
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Keyboard shortcuts only apply to the visible tab.
      if (!isActive || dialog) return;

      const target = e.target as HTMLElement | null;
      const isTyping =
        target?.tagName === "INPUT" || target?.tagName === "TEXTAREA" || target?.isContentEditable;

      if (e.key === "F5") {
        e.preventDefault();
        reloadFolder();
        return;
      }

      if (e.ctrlKey && e.shiftKey && (e.key === "N" || e.key === "n")) {
        e.preventDefault();
        setDialog({ kind: "createFolder" });
        return;
      }

      if (e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        navigateUp(tabId);
        return;
      }

      if (isTyping) return;

      const selectedItem = items.find((item) => selectedItems.has(item.id));
      if (!selectedItem) return;

      if (e.key === "Delete" && selectionMode) {
        e.preventDefault();
        setDialog({ kind: "delete", item: selectedItem });
      } else if (e.key === "F2") {
        e.preventDefault();
        setDialog({ kind: "rename", item: selectedItem });
      } else if (e.key === "Enter") {
        e.preventDefault();
        handleFileOpen(selectedItem);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    dialog,
    isActive,
    items,
    selectedItems,
    selectionMode,
    reloadFolder,
    navigateUp,
    tabId,
    handleFileOpen,
  ]);

  const isSearchActive = searchQuery.trim().length > 0;
  const selectedCount = selectedItems.size;

  return (
    <div
      className="flex flex-col gap-3 h-full relative"
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {/* Drag-and-drop overlay */}
      {isDragOver && (
        <div className="absolute inset-0 z-50 flex items-center justify-center bg-primary/10 border-2 border-dashed border-primary rounded-lg pointer-events-none">
          <div className="flex flex-col items-center gap-2">
            <Upload className="h-12 w-12 text-primary" />
            <span className="text-lg font-medium text-primary">
              {t("files.dropToUpload")}
            </span>
          </div>
        </div>
      )}

      {/* Toolbar */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          {/* Back button: one level up, or back to the account service page at the root */}
          <button
            onClick={() => {
              if (breadcrumbs.length === 0) onExitToHub();
              else navigateUp(tabId);
            }}
            className="rounded-md p-1.5 text-muted-foreground hover:bg-accent transition-colors"
            aria-label={t("files.back")}
            title={t("files.back")}
          >
            <ArrowLeft className="h-4 w-4" />
          </button>

          {/* Breadcrumbs: Explorer-style, leading levels collapse into "…" when long */}
          <FolderBreadcrumb
            driveName={driveName}
            breadcrumbs={breadcrumbs}
            onNavigateRoot={handleNavigateRoot}
            onNavigateIndex={(index) => navigateToBreadcrumb(tabId, index)}
          />

          {/* Refresh current folder */}
          <button
            onClick={reloadFolder}
            className="rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
            aria-label={t("files.refresh")}
            title={t("files.refresh")}
          >
            <RefreshCw className="h-4 w-4" />
          </button>
        </div>

        <div className="flex items-center gap-3">
          <button
            onClick={() => setDialog({ kind: "createFolder" })}
            className="inline-flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
            title={t("files.newFolder")}
          >
            <FolderPlus className="h-4 w-4" />
            <span>{t("files.newFolder")}</span>
          </button>

          {/* Selection mode toggle */}
          <button
            onClick={toggleSelectionMode}
            className={`rounded-md p-1.5 transition-colors ${
              selectionMode
                ? "bg-primary text-primary-foreground"
                : "text-muted-foreground hover:bg-accent"
            }`}
            aria-label={t("files.selectionMode")}
            title={t("files.selectionMode")}
          >
            <CheckSquare className="h-4 w-4" />
          </button>

          {/* Download selected button (visible when items are selected) */}
          {selectionMode && selectedCount > 0 && (
            <button
              onClick={handleDownloadSelected}
              className="flex items-center gap-1.5 rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
              title={t("files.downloadSelected")}
            >
              <Download className="h-3.5 w-3.5" />
              <span>{t("files.downloadSelected")} ({selectedCount})</span>
            </button>
          )}

          {/* Search bar */}
          <SearchBar
            query={searchQuery}
            scope={searchScope}
            onQueryChange={setSearchQuery}
            onScopeChange={setSearchScope}
            onClear={handleClearSearch}
          />

          {/* Layout mode toggles */}
          <div className="flex items-center gap-1 rounded-md border border-border p-0.5">
            <LayoutButton
              mode="list"
              activeMode={layoutMode}
              icon={List}
              label={t("files.layoutList")}
              onClick={(mode) => setTabLayoutMode(tabId, mode)}
            />
            <LayoutButton
              mode="grid"
              activeMode={layoutMode}
              icon={LayoutGrid}
              label={t("files.layoutGrid")}
              onClick={(mode) => setTabLayoutMode(tabId, mode)}
            />
            <LayoutButton
              mode="gallery"
              activeMode={layoutMode}
              icon={GalleryHorizontalEnd}
              label={t("files.layoutGallery")}
              onClick={(mode) => setTabLayoutMode(tabId, mode)}
            />
          </div>
        </div>
      </div>

      {/* Search results replace the normal file listing when search is active */}
      {isSearchActive ? (
        <div className="flex-1 overflow-auto min-h-0">
          <SearchResults
            results={searchResults}
            isSearching={isSearching}
            onNavigateToParent={handleNavigateToParent}
          />
        </div>
      ) : (
        <>
          {/* List header (only in list mode); columns click-toggle sort direction */}
          {layoutMode === "list" && !isLoading && !error && items.length > 0 && (
            <div className="flex items-center gap-3 px-3 py-1 border-b border-border text-xs text-muted-foreground font-medium">
              {selectionMode && <span className="w-4" />}
              <span className="w-5" />
              <SortHeaderButton
                label={t("fileBrowser.name")}
                column="name"
                activeKey={sortKey}
                asc={sortAsc}
                className="flex-1"
                onChange={setTabSort}
                tabId={tabId}
              />
              <SortHeaderButton
                label={t("fileBrowser.size")}
                column="size"
                activeKey={sortKey}
                asc={sortAsc}
                className="w-24 justify-end"
                onChange={setTabSort}
                tabId={tabId}
              />
              <SortHeaderButton
                label={t("fileBrowser.modified")}
                column="modified"
                activeKey={sortKey}
                asc={sortAsc}
                className="w-36 justify-end"
                onChange={setTabSort}
                tabId={tabId}
              />
              {/* Placeholder aligned with the per-row "..." action menu */}
              <span className="w-6 shrink-0" />
            </div>
          )}

          {/* Content area */}
          <div className="flex-1 overflow-auto min-h-0">
            {/* Loading state */}
            {isLoading && (
              <div className="flex items-center justify-center py-16">
                <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
              </div>
            )}

            {/* Error state */}
            {!isLoading && error && (
              <div className="flex flex-col items-center justify-center gap-3 py-16">
                <AlertCircle className="h-8 w-8 text-destructive" />
                <p className="max-w-xl text-center text-sm text-muted-foreground">{error}</p>
                <button
                  onClick={handleRetry}
                  className="rounded-md bg-primary px-3 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
                >
                  {t("errors.retryAction")}
                </button>
              </div>
            )}

            {/* Empty state */}
            {!isLoading && !error && items.length === 0 && (
              <div className="flex flex-col items-center justify-center gap-3 py-16">
                <FolderOpen className="h-10 w-10 text-muted-foreground/50" />
                <p className="text-sm text-muted-foreground">{t("files.emptyFolder")}</p>
              </div>
            )}

            {/* File listing */}
            {!isLoading && !error && items.length > 0 && (
              <>
                {layoutMode === "list" ? (
                  <div className="flex flex-col">
                    {items.map((item) => (
                      <FileItem
                        key={item.id}
                        item={item}
                        layoutMode={layoutMode}
                        driveId={driveId}
                        cloudEnv={cloudEnv}
                        onNavigate={(folderId, folderName) =>
                          navigateToFolder(tabId, folderId, folderName)
                        }
                        selectionMode={selectionMode}
                        isSelected={selectedItems.has(item.id)}
                        onToggleSelect={handleToggleSelect}
                        onAction={handleFileAction}
                        onOpen={handleFileOpen}
                      />
                    ))}
                  </div>
                ) : (
                  <div
                    className={`grid gap-3 ${
                      layoutMode === "grid"
                        ? "grid-cols-[repeat(auto-fill,minmax(140px,1fr))]"
                        : "grid-cols-[repeat(auto-fill,minmax(180px,1fr))]"
                    }`}
                  >
                    {items.map((item) => (
                      <FileItem
                        key={item.id}
                        item={item}
                        layoutMode={layoutMode}
                        driveId={driveId}
                        cloudEnv={cloudEnv}
                        onNavigate={(folderId, folderName) =>
                          navigateToFolder(tabId, folderId, folderName)
                        }
                        selectionMode={selectionMode}
                        isSelected={selectedItems.has(item.id)}
                        onToggleSelect={handleToggleSelect}
                        onAction={handleFileAction}
                        onOpen={handleFileOpen}
                      />
                    ))}
                  </div>
                )}

              </>
            )}
          </div>
        </>
      )}
      {dialog?.kind === "createFolder" && (
        <CreateFolderDialog
          driveId={driveId}
          parentId={currentFolderId ?? "root"}
          cloudEnv={cloudEnv}
          onClose={() => setDialog(null)}
          onSuccess={reloadFolder}
        />
      )}

      {dialog?.kind === "rename" && dialog.item && (
        <RenameDialog
          item={dialog.item}
          driveId={driveId}
          cloudEnv={cloudEnv}
          onClose={() => setDialog(null)}
          onSuccess={reloadFolder}
        />
      )}

      {dialog?.kind === "delete" && dialog.item && (
        <DeleteDialog
          item={dialog.item}
          driveId={driveId}
          cloudEnv={cloudEnv}
          onClose={() => setDialog(null)}
          onSuccess={reloadFolder}
        />
      )}

      {dialog?.kind === "share" && dialog.item && (
        <ShareLinkDialog
          item={dialog.item}
          driveId={driveId}
          cloudEnv={cloudEnv}
          onClose={() => setDialog(null)}
        />
      )}

      {dialog?.kind === "convert" && dialog.item && (
        <ConvertDialog
          item={dialog.item}
          driveId={driveId}
          cloudEnv={cloudEnv}
          onClose={() => setDialog(null)}
        />
      )}

      {dialog?.kind === "properties" && dialog.item && (
        <PropertiesDialog
          item={dialog.item}
          driveId={driveId}
          cloudEnv={cloudEnv}
          onClose={() => setDialog(null)}
        />
      )}
    </div>
  );
}
