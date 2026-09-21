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
  BatchInfo,
  BatchSnapshot,
  CloudEnvironment,
  Drive,
  DriveItem,
  DriveQuota,
  DownloadFileSpec,
  ExternalDownloaderConfig,
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
export function logout(
  cloudEnv: CloudEnvironment,
  homeAccountId?: string
): Promise<void> {
  return invoke<void>("logout", { cloudEnv, homeAccountId });
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
  cloudEnv: CloudEnvironment,
  itemId?: string
): Promise<DriveItem[]> {
  return invoke<DriveItem[]>("search_files", { driveId, query, scope, cloudEnv, itemId });
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
  cloudEnv: CloudEnvironment,
  driveId: string,
  itemId: string,
  format: string,
  savePath: string
): Promise<void> {
  return invoke<void>("convert_format", { cloudEnv, driveId, itemId, format, savePath });
}

/**
 * Get a pre-authenticated preview URL for a file.
 * Supports images and Office documents.
 */
export function getPreviewUrl(
  driveId: string,
  itemId: string,
  cloudEnv: CloudEnvironment
): Promise<string> {
  return invoke<string>("get_preview_url", { driveId, itemId, cloudEnv });
}

/**
 * Get a thumbnail URL for an image or video drive item.
 * Prefers the largest Graph-provided size and falls back to smaller sizes.
 */
export function getThumbnailUrl(
  driveId: string,
  itemId: string,
  cloudEnv: CloudEnvironment
): Promise<string> {
  return invoke<string>("get_thumbnail_url", { driveId, itemId, cloudEnv });
}

/**
 * Get the total size of a drive item.
 * Files return their direct size; folders are summed recursively by the backend.
 */
export function getItemSize(
  driveId: string,
  itemId: string,
  cloudEnv: CloudEnvironment
): Promise<number> {
  return invoke<number>("get_item_size", { driveId, itemId, cloudEnv });
}

/**
 * Get detailed properties/metadata for a file or folder.
 */
export function getItemProperties(
  driveId: string,
  itemId: string,
  cloudEnv: CloudEnvironment
): Promise<DriveItem> {
  return invoke<DriveItem>("get_item_properties", { driveId, itemId, cloudEnv });
}

/**
 * Read a text-based file's content for Markdown/code/plain-text preview.
 */
export function getTextContent(
  driveId: string,
  itemId: string,
  cloudEnv: CloudEnvironment,
  homeAccountId: string
): Promise<string> {
  return invoke<string>("get_text_content", {
    driveId,
    itemId,
    cloudEnv,
    homeAccountId,
  });
}

// ─── SharePoint ─────────────────────────────────────────────────────────────

/**
 * Retrieve SharePoint sites accessible to the user.
 * Uses search for Global, groups-based discovery for China environment.
 */
export function getSharepointSites(
  cloudEnv: CloudEnvironment,
  homeAccountId: string
): Promise<Site[]> {
  return invoke<Site[]>("get_sharepoint_sites", { cloudEnv, homeAccountId });
}

/**
 * Get document libraries (drives) within a SharePoint site.
 */
export function getSiteDrives(
  siteId: string,
  cloudEnv: CloudEnvironment,
  homeAccountId: string
): Promise<Drive[]> {
  return invoke<Drive[]>("get_site_drives", { siteId, cloudEnv, homeAccountId });
}

/**
 * Get drives shared with the current user.
 */
export function getSharedDrives(
  cloudEnv: CloudEnvironment,
  homeAccountId: string
): Promise<Drive[]> {
  return invoke<Drive[]>("get_shared_drives", { cloudEnv, homeAccountId });
}

// ─── Transfers: Downloads ───────────────────────────────────────────────────

/**
 * Start downloading a single file.
 * Returns a task ID for tracking progress.
 */
export function downloadFile(
  driveId: string,
  itemId: string,
  homeAccountId: string,
  fileName: string,
  fileSize: number,
  localPath: string,
  cloudEnv: CloudEnvironment
): Promise<BatchInfo> {
  return invoke<BatchInfo>("download_file", {
    cloudEnv,
    driveId,
    itemId,
    homeAccountId,
    fileName,
    fileSize,
    localPath,
  });
}

/**
 * Start downloading an entire folder recursively.
 * Returns a task ID for tracking progress.
 */
export function downloadFolder(
  driveId: string,
  itemId: string,
  localPath: string,
  cloudEnv: CloudEnvironment,
  homeAccountId: string,
  batchName: string
): Promise<BatchInfo> {
  return invoke<BatchInfo>("download_folder", {
    driveId,
    itemId,
    localPath,
    cloudEnv,
    homeAccountId,
    batchName,
  });
}

/**
 * Start downloading multiple selected files into one local directory as a batch.
 */
export function downloadFiles(
  driveId: string,
  homeAccountId: string,
  files: DownloadFileSpec[],
  localDir: string,
  cloudEnv: CloudEnvironment,
  batchName: string
): Promise<BatchInfo> {
  return invoke<BatchInfo>("download_files", {
    driveId,
    homeAccountId,
    files,
    localDir,
    cloudEnv,
    batchName,
  });
}

/**
 * Load persisted download batches after app restart.
 */
export function getDownloadTasks(): Promise<BatchSnapshot[]> {
  return invoke<BatchSnapshot[]>("get_download_tasks");
}

/** Pause an active download task. */
export function pauseDownload(taskId: string): Promise<void> {
  return invoke<void>("pause_download", { taskId });
}

/** Resume a paused download task. */
export function resumeDownload(
  cloudEnv: CloudEnvironment,
  taskId: string
): Promise<void> {
  return invoke<void>("resume_download", { cloudEnv, taskId });
}

/** Cancel a download task and clean up partial files. */
export function cancelDownload(taskId: string): Promise<void> {
  return invoke<void>("cancel_download", { taskId });
}

/** Remove a download task from the list. Completed files are kept on disk. */
export function removeDownloadTask(taskId: string): Promise<void> {
  return invoke<void>("remove_download", { taskId });
}

// ─── Transfers: Uploads ─────────────────────────────────────────────────────

/**
 * Upload one or more files to a target folder.
 * Returns an array of task IDs for tracking progress.
 */
export function uploadFiles(
  driveId: string,
  parentId: string,
  filePaths: string[],
  cloudEnv: CloudEnvironment
): Promise<string[]> {
  return invoke<string[]>("upload_files", { driveId, parentId, filePaths, cloudEnv });
}

/**
 * Upload a local folder (recursively) to a target cloud folder.
 * Returns a task ID for tracking progress.
 */
export function uploadFolder(
  driveId: string,
  parentId: string,
  folderPath: string,
  cloudEnv: CloudEnvironment
): Promise<string[]> {
  return invoke<string[]>("upload_folder", { driveId, parentId, folderPath, cloudEnv });
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
export function getDriveQuota(
  driveId: string,
  cloudEnv: CloudEnvironment
): Promise<DriveQuota> {
  return invoke<DriveQuota>("get_drive_quota", { driveId, cloudEnv });
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
