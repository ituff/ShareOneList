/**
 * Frontend TypeScript types mirroring the Rust backend data models.
 * These interfaces are serialized/deserialized across the Tauri IPC bridge.
 * Property names use camelCase; the Rust models use serde rename_all = "camelCase".
 */

// ─── File & Drive Models ────────────────────────────────────────────────────

/** A reference to the parent item in the drive hierarchy. */
export interface ParentReference {
  driveId: string;
  id: string;
  path: string | null;
  name: string | null;
}

/** Represents a file or folder in OneDrive/SharePoint. */
export interface DriveItem {
  id: string;
  name: string;
  size: number | null;
  lastModified: string;
  isFolder: boolean;
  mimeType: string | null;
  webUrl: string | null;
  parentReference: ParentReference | null;
  downloadUrl: string | null;
  createdDateTime: string | null;
}

/** Represents a OneDrive or SharePoint document library drive. */
export interface Drive {
  id: string;
  name: string;
  driveType: string;
  quota: DriveQuota | null;
}

/** Storage quota information for a drive. */
export interface DriveQuota {
  total: number;
  used: number;
  remaining: number;
}

/** Represents a SharePoint site. */
export interface Site {
  id: string;
  displayName: string;
  webUrl: string;
}

/** Whether a Teams recording is the user's own or shared with them. */
export type RecordingSource = "own" | "shared";

/** A Teams meeting recording aggregated across the user's drives. */
export interface MeetingRecording {
  /** Drive holding the recording file (used for thumbnails/downloads). */
  driveId: string;
  /** The recording file itself. */
  item: DriveItem;
  sourceType: RecordingSource;
  /** Originating site name; empty for OneDrive recordings. */
  sourceName: string;
}

// ─── Navigation & UI State ──────────────────────────────────────────────────

/** A single breadcrumb entry for folder path navigation. */
export interface BreadcrumbItem {
  id: string;
  name: string;
}

/** The cloud environment type for authentication and API routing. */
export type CloudEnvironment = "global" | "china";

/** Layout mode for the file browser view. */
export type LayoutMode = "list" | "grid" | "gallery";

/** File list sort column. */
export type SortKey = "name" | "size" | "modified";

/** Per-tab state for multi-tab file browsing. */
export interface TabState {
  id: string;
  /** Whether this tab browses a drive, previews a file, or lists meeting recordings. */
  kind: "drive" | "preview" | "recordings";
  driveId: string;
  driveName: string;
  cloudEnv: CloudEnvironment;
  homeAccountId: string;
  /** The file being previewed when kind is "preview". */
  previewItem?: DriveItem;
  currentFolderId: string;
  breadcrumbs: BreadcrumbItem[];
  items: DriveItem[];
  /** Current sort column and direction for the file listing. */
  sortKey: SortKey;
  sortAsc: boolean;
  layoutMode: LayoutMode;
  isLoading: boolean;
  error: string | null;
}

// ─── Configuration ──────────────────────────────────────────────────────────

/** Theme mode preference. */
export type ThemeMode = "light" | "dark" | "system";

/** Persisted window position and dimensions. */
export interface WindowState {
  x: number;
  y: number;
  width: number;
  height: number;
  isMaximized: boolean;
}

/** Application-level configuration stored on disk. */
export interface AppConfig {
  theme: ThemeMode;
  language: string;
  window: WindowState;
  /** Last directory used by the native save dialog for downloads. */
  lastDownloadPath: string | null;
  /** Concurrent segment fetches for the recording stream pipeline (1-16). */
  segmentDownloadConcurrency: number;
}

/** A persisted account entry linking a user to a specific drive. */
/** Whether a Microsoft identity is a consumer or a work/school account. */
export type AccountType = "personal" | "organization";

export interface AccountEntry {
  homeAccountId: string;
  driveId: string;
  cloudType: CloudEnvironment;
  displayName: string;
  /** Personal vs organizational; global accounts only, `null` for legacy entries. */
  accountType?: AccountType | null;
  /** User-set alias shown instead of the display name; absent when unset. */
  alias?: string | null;
  /** Identifier of the user-chosen icon from the built-in icon library. */
  icon?: string | null;
}

/** Result returned when a download batch is created. */
export interface BatchInfo {
  batchId: string;
  batchName: string;
}

/** A single file to include in a batch download. */
export interface DownloadFileSpec {
  itemId: string;
  fileName: string;
  fileSize: number;
}

/** A user bookmark pointing to a file or folder in a drive. */
export interface BookmarkEntry {
  id: string;
  name: string;
  driveId: string;
  driveName: string;
  itemId: string;
  cloudEnv: CloudEnvironment;
  homeAccountId: string;
  isFolder: boolean;
  createdAt: string;
}

/** Snapshot of a persisted download batch used to restore tasks on startup. */
export interface BatchSnapshot {
  id: string;
  name: string;
  status: TaskStatus;
  totalBytes: number;
  downloadedBytes: number;
  speedBps: number;
  elapsedSecs: number;
  error: string | null;
  localPath: string;
  cloudEnv: CloudEnvironment;
  driveId: string;
  homeAccountId: string;
}

// ─── Account Info ───────────────────────────────────────────────────────────

/** Information returned after a successful login. */
export interface AccountInfo {
  homeAccountId: string;
  displayName: string;
  driveId: string;
  cloudEnv: CloudEnvironment;
  /** Personal vs organizational; global accounts only. */
  accountType?: AccountType | null;
}

// ─── File Operations ────────────────────────────────────────────────────────

/** Options for creating a share link. */
export interface ShareOptions {
  linkType: "view" | "edit";
  expiration: string | null;
  password: string | null;
}

/** Properties/metadata of a drive item. */
export interface ItemProperties {
  name: string;
  size: number | null;
  createdDateTime: string | null;
  lastModifiedDateTime: string | null;
  webUrl: string | null;
}

// ─── Transfer & Progress ────────────────────────────────────────────────────

/** Status of a transfer task. */
export type TaskStatus =
  | "queued"
  | "downloading"
  | "uploading"
  | "paused"
  | "completed"
  | "failed";

/** Progress event emitted by the backend during downloads/uploads. */
export interface ProgressEvent {
  taskId: string;
  fileName: string;
  status: TaskStatus;
  totalBytes: number;
  transferredBytes: number;
  speedBps: number;
  elapsedSecs: number;
  error: string | null;
  /** Actual local path of the transferred file (downloads only). */
  localPath: string | null;
}

// ─── External Downloader ────────────────────────────────────────────────────

/** Supported external downloader types. */
export type DownloaderType = "aria2" | "motrix" | "idm";

/** Configuration for pushing a download to an external downloader. */
export interface ExternalDownloaderConfig {
  downloaderType: DownloaderType;
  rpcUrl: string;
  secret: string | null;
  downloadUrl: string;
  fileName: string;
}

// ─── Application Update ─────────────────────────────────────────────────────

/** Information about an available application update. */
export interface UpdateInfo {
  version: string;
  changelog: string;
  downloadUrl: string;
}

// ─── Search ─────────────────────────────────────────────────────────────────

/** Search scope: current folder subtree or entire drive. */
export type SearchScope = "local" | "global";
