/**
 * Typed Tauri invoke wrappers for all IPC commands.
 * Each function maps to a #[tauri::command] handler in the Rust backend.
 * Uses @tauri-apps/api/core for Tauri 2.x invoke API.
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  AccountEntry,
  AccountInfo,
  AppConfig,
  CloudEnvironment,
  Drive,
  DriveItem,
  DriveQuota,
  ExternalDownloaderConfig,
  ItemProperties,
  SearchScope,
  ShareOptions,
  Site,
  UpdateInfo,
} from "./types";

// ─── Authentication ─────────────────────────────────────────────────────────

/**
 * Initiate OAuth2 login for the specified cloud environment.
 * Opens system browser for authorization and returns account info on success.
 */
export function login(cloudEnv: CloudEnvironment): Promise<AccountInfo> {
  return invoke<AccountInfo>("login", { cloudEnv });
}

/**
 * Log out from the specified cloud environment.
 * Clears tokens from secure storage.
 */
export function logout(cloudEnv: CloudEnvironment): Promise<void> {
  return invoke<void>("logout", { cloudEnv });
}

// ─── File Operations ────────────────────────────────────────────────────────

/**
 * List children (files and folders) of a specific folder in a drive.
 * Handles pagination internally and returns the complete list.
 */
export function listFiles(
  driveId: string,
  itemId: string,
  cloudEnv: CloudEnvironment
): Promise<DriveItem[]> {
  return invoke<DriveItem[]>("list_files", { driveId, itemId, cloudEnv });
}

/**
 * Search for files matching a query within a drive.
 * Scope can be "local" (current folder subtree) or "global" (entire drive).
 */
export function searchFiles(
  driveId: string,
  query: string,
  scope: SearchScope,
  itemId?: string
): Promise<DriveItem[]> {
  return invoke<DriveItem[]>("search_files", { driveId, query, scope, itemId });
}

/**
 * Create a new folder inside the specified parent folder.
 * Returns the newly created folder item.
 */
export function createFolder(
  driveId: string,
  parentId: string,
  name: string,
  cloudEnv: CloudEnvironment
): Promise<DriveItem> {
  return invoke<DriveItem>("create_folder", { driveId, parentId, name, cloudEnv });
}

/**
 * Rename a file or folder.
 * Returns the updated item with the new name.
 */
export function renameItem(
  driveId: string,
  itemId: string,
  newName: string,
  cloudEnv: CloudEnvironment
): Promise<DriveItem> {
  return invoke<DriveItem>("rename_item", { driveId, itemId, newName, cloudEnv });
}

/**
 * Delete a file or folder (soft delete to recycle bin).
 */
export function deleteItem(
  driveId: string,
  itemId: string,
  cloudEnv: CloudEnvironment
): Promise<void> {
  return invoke<void>("delete_item", { driveId, itemId, cloudEnv });
}

/**
 * Create a sharing link for a file or folder.
 * Returns the generated share URL.
 */
export function createShareLink(
  driveId: string,
  itemId: string,
  options: ShareOptions,
  cloudEnv: CloudEnvironment
): Promise<string> {
  return invoke<string>("create_share_link", { driveId, itemId, options, cloudEnv });
}

/**
 * Convert a document to the specified format (e.g., PDF).
 * The converted file is saved to the given local path.
 */
export function convertFormat(
  driveId: string,
  itemId: string,
  format: string,
  savePath: string
): Promise<void> {
  return invoke<void>("convert_format", { driveId, itemId, format, savePath });
}

/**
 * Get a pre-authenticated preview URL for a file.
 * Supports images and Office documents.
 */
export function getPreviewUrl(
  driveId: string,
  itemId: string
): Promise<string> {
  return invoke<string>("get_preview_url", { driveId, itemId });
}

/**
 * Get detailed properties/metadata for a file or folder.
 */
export function getItemProperties(
  driveId: string,
  itemId: string
): Promise<ItemProperties> {
  return invoke<ItemProperties>("get_item_properties", { driveId, itemId });
}

// ─── SharePoint ─────────────────────────────────────────────────────────────

/**
 * Retrieve SharePoint sites accessible to the user.
 * Uses search for Global, groups-based discovery for China environment.
 */
export function getSharepointSites(
  cloudEnv: CloudEnvironment
): Promise<Site[]> {
  return invoke<Site[]>("get_sharepoint_sites", { cloudEnv });
}

/**
 * Get document libraries (drives) within a SharePoint site.
 */
export function getSiteDrives(siteId: string): Promise<Drive[]> {
  return invoke<Drive[]>("get_site_drives", { siteId });
}

/**
 * Get drives shared with the current user.
 */
export function getSharedDrives(): Promise<Drive[]> {
  return invoke<Drive[]>("get_shared_drives");
}

// ─── Transfers: Downloads ───────────────────────────────────────────────────

/**
 * Start downloading a single file.
 * Returns a task ID for tracking progress.
 */
export function downloadFile(
  driveId: string,
  itemId: string,
  localPath: string
): Promise<string> {
  return invoke<string>("download_file", { driveId, itemId, localPath });
}

/**
 * Start downloading an entire folder recursively.
 * Returns a task ID for tracking progress.
 */
export function downloadFolder(
  driveId: string,
  itemId: string,
  localPath: string
): Promise<string> {
  return invoke<string>("download_folder", { driveId, itemId, localPath });
}

/** Pause an active download task. */
export function pauseDownload(taskId: string): Promise<void> {
  return invoke<void>("pause_download", { taskId });
}

/** Resume a paused download task. */
export function resumeDownload(taskId: string): Promise<void> {
  return invoke<void>("resume_download", { taskId });
}

/** Cancel a download task and clean up partial files. */
export function cancelDownload(taskId: string): Promise<void> {
  return invoke<void>("cancel_download", { taskId });
}

// ─── Transfers: Uploads ─────────────────────────────────────────────────────

/**
 * Upload one or more files to a target folder.
 * Returns an array of task IDs for tracking progress.
 */
export function uploadFiles(
  driveId: string,
  parentId: string,
  filePaths: string[]
): Promise<string[]> {
  return invoke<string[]>("upload_files", { driveId, parentId, filePaths });
}

/**
 * Upload a local folder (recursively) to a target cloud folder.
 * Returns a task ID for tracking progress.
 */
export function uploadFolder(
  driveId: string,
  parentId: string,
  folderPath: string
): Promise<string> {
  return invoke<string>("upload_folder", { driveId, parentId, folderPath });
}

/** Cancel an upload task. */
export function cancelUpload(taskId: string): Promise<void> {
  return invoke<void>("cancel_upload", { taskId });
}

// ─── File System ────────────────────────────────────────────────────────────

/**
 * Open the native file explorer with the specified file selected.
 * Uses explorer.exe /select on Windows, open -R on macOS.
 */
export function openContainingFolder(path: string): Promise<void> {
  return invoke<void>("open_containing_folder", { path });
}

// ─── Storage ────────────────────────────────────────────────────────────────

/** Get storage quota information for a specific drive. */
export function getDriveQuota(driveId: string): Promise<DriveQuota> {
  return invoke<DriveQuota>("get_drive_quota", { driveId });
}

// ─── External Downloader ────────────────────────────────────────────────────

/**
 * Push a download task to an external downloader (Aria2, Motrix, or IDM).
 * Sends a JSON-RPC request or command-line invocation depending on type.
 */
export function pushToDownloader(
  config: ExternalDownloaderConfig
): Promise<void> {
  return invoke<void>("push_to_downloader", { config });
}

/**
 * Parse a SharePoint sharing URL and extract a direct download link.
 * Returns null if the URL is not a valid SharePoint sharing link.
 */
export function parseSharepointUrl(url: string): Promise<string | null> {
  return invoke<string | null>("parse_sharepoint_url", { url });
}

// ─── Settings ───────────────────────────────────────────────────────────────

/** Load the application configuration from disk. */
export function getConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("get_config");
}

/** Persist the application configuration to disk. */
export function saveConfig(config: AppConfig): Promise<void> {
  return invoke<void>("save_config", { config });
}

/** Load the list of saved account entries. */
export function getAccounts(): Promise<AccountEntry[]> {
  return invoke<AccountEntry[]>("get_accounts");
}

// ─── Application Update ─────────────────────────────────────────────────────

/**
 * Check GitHub releases for a newer version.
 * Returns update info if available, null if already up to date.
 */
export function checkUpdate(): Promise<UpdateInfo | null> {
  return invoke<UpdateInfo | null>("check_update");
}

/**
 * Download and install a specific version update.
 * Downloads the platform-appropriate archive and applies it.
 */
export function performUpdate(version: string): Promise<void> {
  return invoke<void>("perform_update", { version });
}
