/**
 * Frontend TypeScript types mirroring the Rust backend data models.
 * These interfaces are serialized/deserialized across the Tauri IPC bridge.
 * Property names use camelCase — Tauri's serde rename handles snake_case conversion.
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

/** Per-tab state for multi-tab file browsing. */
export interface TabState {
  id: string;
  driveId: string;
  driveName: string;
  cloudEnv: CloudEnvironment;
  currentFolderId: string;
  breadcrumbs: BreadcrumbItem[];
  items: DriveItem[];
  layoutMode: LayoutMode;
  isLoading: boolean;
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
}

/** A persisted account entry linking a user to a specific drive. */
export interface AccountEntry {
  homeAccountId: string;
  driveId: string;
  cloudType: CloudEnvironment;
  displayName: string;
}

// ─── Account Info ───────────────────────────────────────────────────────────

/** Information returned after a successful login. */
export interface AccountInfo {
  homeAccountId: string;
  displayName: string;
  driveId: string;
  cloudEnv: CloudEnvironment;
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
