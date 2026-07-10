import { useEffect, useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  List,
  LayoutGrid,
  GalleryHorizontalEnd,
  Loader2,
  FolderOpen,
  AlertCircle,
  ChevronRight,
  Home,
  ArrowLeft,
  CheckSquare,
  Download,
  Upload,
} from "lucide-react";
import { useFileStore } from "../../stores/fileStore";
import { FileItem } from "./FileItem";
import { SearchBar } from "./SearchBar";
import { SearchResults } from "./SearchResults";
import { searchFiles, uploadFiles, downloadFile } from "../../lib/tauri";
import type { CloudEnvironment, DriveItem, LayoutMode, SearchScope } from "../../lib/types";

interface FileBrowserProps {
  driveId: string;
  cloudEnv: CloudEnvironment;
  driveName: string;
  /** Called when the user wants to go back to drive selection. */
  onBack?: () => void;
}

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

export function FileBrowser({ driveId, cloudEnv, driveName, onBack }: FileBrowserProps) {
  const { t } = useTranslation();
  const {
    items,
    isLoading,
    error,
    breadcrumbs,
    layoutMode,
    hasMore,
    currentFolderId,
    loadFolder,
    navigateToFolder,
    navigateToBreadcrumb,
    loadMore,
    setLayoutMode,
  } = useFileStore();

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

  // Load root folder on mount
  useEffect(() => {
    loadFolder(driveId, "root", cloudEnv);
  }, [driveId, cloudEnv, loadFolder]);

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
  }, [driveId, currentFolderId]);

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

  /** Handle file drop: upload dropped files to current folder. */
  const handleFileDrop = useCallback(
    async (filePaths: string[]) => {
      if (!currentFolderId) return;
      try {
        await uploadFiles(driveId, currentFolderId, filePaths);
        // Refresh folder after upload starts
        loadFolder(driveId, currentFolderId, cloudEnv);
      } catch {
        // Upload errors are handled by TaskManager
      }
    },
    [driveId, currentFolderId, cloudEnv, loadFolder]
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
  }, [searchQuery, searchScope, driveId, currentFolderId]);

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
        navigateToFolder(parentId, parentName ?? "Folder");
      }
    },
    [navigateToFolder, handleClearSearch]
  );

  const handleRetry = () => {
    const state = useFileStore.getState();
    if (state.driveId && state.currentFolderId && state.currentCloudEnv) {
      loadFolder(state.driveId, state.currentFolderId, state.currentCloudEnv);
    }
  };

  /** Navigate to root when clicking the home breadcrumb. */
  const handleNavigateRoot = () => {
    loadFolder(driveId, "root", cloudEnv);
    // Clear breadcrumbs (root is the start)
    useFileStore.setState({ breadcrumbs: [] });
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

  /** Download all selected items. */
  const handleDownloadSelected = useCallback(async () => {
    const selectedFiles = items.filter(
      (item) => selectedItems.has(item.id) && !item.isFolder
    );
    for (const file of selectedFiles) {
      try {
        // Use file name as local path — the backend will prompt or use a default download dir
        await downloadFile(driveId, file.id, file.name);
      } catch {
        // Errors handled by TaskManager
      }
    }
    // Clear selection after initiating downloads
    setSelectedItems(new Set());
    setSelectionMode(false);
  }, [items, selectedItems, driveId]);

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
          {/* Back button */}
          {onBack && (
            <button
              onClick={onBack}
              className="rounded-md p-1.5 text-muted-foreground hover:bg-accent transition-colors"
              aria-label={t("files.back")}
              title={t("files.back")}
            >
              <ArrowLeft className="h-4 w-4" />
            </button>
          )}

          {/* Breadcrumbs */}
          <nav className="flex items-center gap-1 text-sm" aria-label="Breadcrumb">
            <button
              onClick={handleNavigateRoot}
              className="flex items-center gap-1 rounded px-1.5 py-0.5 text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
            >
              <Home className="h-3.5 w-3.5" />
              <span>{driveName}</span>
            </button>
            {breadcrumbs.map((crumb, index) => (
              <span key={crumb.id} className="flex items-center gap-1">
                <ChevronRight className="h-3 w-3 text-muted-foreground" />
                <button
                  onClick={() => navigateToBreadcrumb(index)}
                  className="rounded px-1.5 py-0.5 text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
                >
                  {crumb.name}
                </button>
              </span>
            ))}
          </nav>
        </div>

        <div className="flex items-center gap-3">
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
              onClick={setLayoutMode}
            />
            <LayoutButton
              mode="grid"
              activeMode={layoutMode}
              icon={LayoutGrid}
              label={t("files.layoutGrid")}
              onClick={setLayoutMode}
            />
            <LayoutButton
              mode="gallery"
              activeMode={layoutMode}
              icon={GalleryHorizontalEnd}
              label={t("files.layoutGallery")}
              onClick={setLayoutMode}
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
          {/* List header (only in list mode) */}
          {layoutMode === "list" && !isLoading && !error && items.length > 0 && (
            <div className="flex items-center gap-3 px-3 py-1 border-b border-border text-xs text-muted-foreground font-medium">
              {selectionMode && <span className="w-4" />}
              <span className="w-5" />
              <span className="flex-1">{t("fileBrowser.name")}</span>
              <span className="w-24 text-right">{t("fileBrowser.size")}</span>
              <span className="w-36 text-right">{t("fileBrowser.modified")}</span>
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
                <p className="text-sm text-muted-foreground">{t("errors.loadFailed")}</p>
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
                        onNavigate={navigateToFolder}
                        selectionMode={selectionMode}
                        isSelected={selectedItems.has(item.id)}
                        onToggleSelect={handleToggleSelect}
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
                        onNavigate={navigateToFolder}
                        selectionMode={selectionMode}
                        isSelected={selectedItems.has(item.id)}
                        onToggleSelect={handleToggleSelect}
                      />
                    ))}
                  </div>
                )}

                {/* Load more button for pagination */}
                {hasMore && (
                  <div className="flex justify-center py-4">
                    <button
                      onClick={loadMore}
                      className="rounded-md border border-border px-4 py-2 text-sm text-muted-foreground hover:bg-accent transition-colors"
                    >
                      {t("files.loadMore")}
                    </button>
                  </div>
                )}
              </>
            )}
          </div>
        </>
      )}
    </div>
  );
}
