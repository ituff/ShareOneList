# Requirements Document

## Introduction

ShareOneList is being rewritten from a WinUI 3 (.NET/C#) application to a Tauri-based cross-platform application (Rust backend + web frontend). The rewrite preserves all existing functionality while enabling support for both Windows and macOS (Apple Silicon). The application provides unified file management for Microsoft 365 OneDrive and SharePoint, supporting both Global (International) and 21Vianet (China) cloud environments.

## Glossary

- **App**: The ShareOneList Tauri application
- **Auth_Module**: The authentication module responsible for OAuth2 flows and token management
- **Graph_Client**: The module that communicates with Microsoft Graph API endpoints
- **File_Browser**: The UI component for browsing and navigating files and folders
- **Task_Manager**: The module that tracks and manages download and upload operations
- **Drive_Manager**: The module that manages drive connections across multiple accounts
- **Settings_Store**: The persistent local storage for user preferences and configuration
- **Token_Cache**: The secure local storage for OAuth2 refresh tokens and session data
- **i18n_Module**: The internationalization module for multi-language support
- **Theme_Engine**: The module responsible for dark/light theme switching
- **External_Downloader**: The tool that generates download configurations for third-party downloaders (Aria2)
- **Cloud_Environment**: Either Global (International Microsoft 365) or China (21Vianet Microsoft 365)
- **Preview_Module**: The module for in-app file previewing (images, markdown, media)

## Requirements

### Requirement 1: Cross-Platform Application Shell

**User Story:** As a user, I want to run ShareOneList on both Windows and macOS (Apple Silicon), so that I can manage my cloud files regardless of my operating system.

#### Acceptance Criteria

1. THE App SHALL build and run natively on Windows (x64, ARM64) and macOS (Apple Silicon / aarch64)
2. THE App SHALL use Tauri as the application framework with a Rust backend and a web-based frontend
3. THE App SHALL provide a native window with system-appropriate title bar and window controls on each platform
4. THE App SHALL support window resizing with a minimum window size of 800x600 pixels, minimizing, maximizing, and closing on both platforms
5. WHEN the App is launched and a valid previous window state exists in Settings_Store, THE App SHALL restore the previous window size and position
6. IF the App is launched with no saved window state in Settings_Store, THEN THE App SHALL display the window centered on the primary display with a default size of 1024x768 pixels
7. IF the App is launched and the saved window position is outside the bounds of all currently connected displays, THEN THE App SHALL reposition the window to the center of the primary display while preserving the saved window size

### Requirement 2: OAuth2 Authentication

**User Story:** As a user, I want to sign in with my Microsoft account, so that I can access my OneDrive and SharePoint files.

#### Acceptance Criteria

1. WHEN a user initiates login, THE Auth_Module SHALL open a system browser or embedded webview for OAuth2 authorization code flow with PKCE
2. THE Auth_Module SHALL support Azure AD endpoints for both Global (login.microsoftonline.com) and China (login.partner.microsoftonline.cn) Cloud_Environments
3. WHEN OAuth2 authorization completes, THE Auth_Module SHALL exchange the authorization code for access and refresh tokens within 30 seconds of receiving the code
4. THE Auth_Module SHALL store refresh tokens securely in the Token_Cache using platform-appropriate secure storage (Windows Credential Manager or macOS Keychain)
5. WHEN an access token has 5 minutes or less of remaining validity, THE Auth_Module SHALL proactively refresh it using the stored refresh token without displaying any UI or requiring user action
6. IF token refresh fails OR IF initial token exchange fails due to network error or server rejection, THEN THE Auth_Module SHALL prompt the user to re-authenticate interactively
7. THE Auth_Module SHALL request the scopes: User.Read, Files.ReadWrite.All, Sites.Read.All, Group.Read.All (with appropriate URI prefix for China environment)
8. THE Auth_Module SHALL support at least 2 concurrent accounts (one per Cloud_Environment)
9. IF a user cancels or closes the login browser/webview before completing authorization, THEN THE Auth_Module SHALL abort the authentication flow and return the user to the previous screen without modifying the Token_Cache

### Requirement 3: Dual Cloud Environment Support

**User Story:** As a user, I want to access both Global and 21Vianet Microsoft 365, so that I can manage files in either environment from one application.

#### Acceptance Criteria

1. THE App SHALL support configuring separate Azure AD Client IDs for Global and China Cloud_Environments via a JSON configuration file with keys nested under an "AzureAD" section (e.g., AzureAD.Global.ClientId, AzureAD.China.ClientId)
2. WHEN connecting to the China Cloud_Environment, THE Graph_Client SHALL use the base URL https://microsoftgraph.chinacloudapi.cn/v1.0
3. WHEN connecting to the Global Cloud_Environment, THE Graph_Client SHALL use the base URL https://graph.microsoft.com/v1.0
4. THE App SHALL maintain independent authentication sessions for each Cloud_Environment such that token refresh, expiration, or authentication failure in one Cloud_Environment does not affect the authentication state of the other Cloud_Environment
5. THE App SHALL display the Cloud_Environment type label ("Global" or "China") next to each connected account's display name in the account list
6. IF the Client ID for a selected Cloud_Environment is missing or empty in the configuration file, THEN THE App SHALL prevent login for that Cloud_Environment and display an error message indicating the missing configuration
7. THE App SHALL allow the user to be concurrently authenticated in both Cloud_Environments within the same application session

### Requirement 4: Drive and Account Management

**User Story:** As a user, I want to add, view, and remove multiple cloud accounts, so that I can manage files across all my Microsoft 365 subscriptions.

#### Acceptance Criteria

1. THE Drive_Manager SHALL display a list of all connected accounts with their display names and Cloud_Environment types
2. WHEN a user adds a new account, THE Drive_Manager SHALL initiate the OAuth2 login flow for the selected Cloud_Environment
3. WHEN login succeeds, THE Drive_Manager SHALL persist the account information (home account ID, drive ID, cloud type) to Settings_Store and update the displayed account list
4. IF login fails or the user cancels the OAuth2 flow, THEN THE Drive_Manager SHALL display an error message indicating the failure reason and leave the existing account list unchanged
5. IF a user attempts to add an account whose home account ID already exists in Settings_Store, THEN THE Drive_Manager SHALL reject the addition and display a message indicating the account is already connected
6. WHEN a user removes an account, THE Drive_Manager SHALL prompt for confirmation, and upon confirmation, clear the associated tokens from Token_Cache and remove the account from Settings_Store
7. THE Drive_Manager SHALL cache the drive list locally and display the cached account list on startup before completing any network refresh

### Requirement 5: OneDrive File Browsing

**User Story:** As a user, I want to browse my OneDrive files and folders, so that I can find and manage my cloud-stored documents.

#### Acceptance Criteria

1. WHEN a user selects their OneDrive, THE File_Browser SHALL retrieve and display the root folder contents via Graph API, requesting up to 200 items per page and providing a mechanism to load additional pages if more items exist
2. WHEN a user double-clicks a folder, THE File_Browser SHALL navigate into that folder and display its contents
3. THE File_Browser SHALL display file name, size (in human-readable format: bytes, KB, MB, GB, TB), last modified date, and file type icon for each item
4. THE File_Browser SHALL display a breadcrumb navigation bar showing the current path hierarchy, with the root (drive name) as the first item
5. WHEN a user clicks a breadcrumb item, THE File_Browser SHALL navigate to that folder level and update the displayed contents accordingly
6. WHEN a user presses Backspace and the current folder is not the root folder, THE File_Browser SHALL navigate to the parent folder
7. THE File_Browser SHALL support three layout modes: list view, grid view, and image gallery view
8. THE File_Browser SHALL display folder items before file items in the listing, with items sorted alphabetically by name within each group
9. IF the Graph API request for folder contents fails, THEN THE File_Browser SHALL display an error message indicating the failure reason and retain the user's current navigation position
10. WHEN a folder contains no items, THE File_Browser SHALL display an empty-state indicator communicating that the folder is empty

### Requirement 6: SharePoint Site and Library Browsing

**User Story:** As a user, I want to browse SharePoint sites and document libraries, so that I can access team files stored in SharePoint.

#### Acceptance Criteria

1. WHEN a user selects SharePoint for a Global account, THE Graph_Client SHALL retrieve available sites using the /sites?search=* endpoint and THE File_Browser SHALL display the returned sites as a selectable list showing each site's display name
2. WHEN a user selects SharePoint for a China account, THE Graph_Client SHALL retrieve sites via the user's M365 Groups (/me/memberOf → Unified Groups → /groups/{id}/sites/root) and THE File_Browser SHALL display the returned sites as a selectable list showing each site's display name
3. WHEN a user selects a SharePoint site, THE File_Browser SHALL retrieve and display the document libraries (drives) for that site, showing each library's name and storage usage
4. WHEN a user selects a document library, THE File_Browser SHALL navigate into it using the same browsing interface as OneDrive (file listing, breadcrumb navigation, layout modes as defined in Requirement 5)
5. IF the connected account has a OneDrive license, THEN THE App SHALL allow browsing shared drives accessible via /me/drive/sharedWithMe, displaying them as a selectable list of drives
6. IF site retrieval fails due to a network error or insufficient permissions, THEN THE File_Browser SHALL display an error message indicating the failure reason and allow the user to retry the request
7. IF no SharePoint sites are found for the account (zero results from Global search or no Unified Groups for China), THEN THE File_Browser SHALL display an empty-state message indicating no sites are available

### Requirement 7: File Download

**User Story:** As a user, I want to download files and folders from the cloud, so that I can access them offline on my local machine.

#### Acceptance Criteria

1. WHEN a user initiates a file download, THE Task_Manager SHALL create a download task and begin transferring the file to the user-selected local path
2. THE Task_Manager SHALL download files using 8 parallel chunks, each up to 1 MB in size
3. WHEN a user initiates a folder download, THE Task_Manager SHALL recursively download all files and subfolders preserving the directory structure
4. WHILE a download task is active, THE Task_Manager SHALL update and display progress (percentage, downloaded bytes, total bytes, speed) at least once per second
5. THE Task_Manager SHALL support pausing and resuming download tasks
6. IF a download is resumed and the elapsed time since the download URL was obtained exceeds 1 hour, THEN THE Task_Manager SHALL request a fresh download URL from Graph API before continuing the transfer
7. THE Task_Manager SHALL support batch downloads when multiple items are selected via checkboxes
8. IF all download tasks have completed and the shutdown-after-download option is enabled, THEN THE App SHALL initiate system shutdown
9. IF a download fails due to a network error or server error, THEN THE Task_Manager SHALL pause the affected task and display an error indication to the user, preserving any already-downloaded data for later resume
10. IF the target local path already contains a file with the same name, THEN THE Task_Manager SHALL create the file with a renamed suffix to avoid overwriting the existing file

### Requirement 8: File Upload

**User Story:** As a user, I want to upload files and folders to the cloud, so that I can store and share my local files.

#### Acceptance Criteria

1. WHEN a user selects files for upload, THE Task_Manager SHALL upload each file to the current folder using Graph API simple upload for files up to 4 MB and upload sessions with 320 KB chunks for files larger than 4 MB
2. THE Task_Manager SHALL display upload progress for each upload task showing percentage complete, uploaded bytes, total bytes, and transfer speed, updated at least once per second
3. WHEN a user uploads a folder, THE Task_Manager SHALL create the folder structure in the cloud and upload all contained files recursively
4. WHEN a user drags files onto the file browser, THE App SHALL initiate upload of the dropped files to the current folder
5. WHEN an upload completes, THE File_Browser SHALL refresh the current folder listing
6. IF a file with the same name already exists in the target folder, THEN THE Task_Manager SHALL use rename conflict behavior to upload the file with a system-generated unique name
7. IF a chunk upload fails due to a network error or server error, THEN THE Task_Manager SHALL retry the failed chunk up to 3 times with exponential backoff before marking the upload task as failed and displaying an error message indicating the failure reason
8. IF an upload session creation fails, THEN THE Task_Manager SHALL display an error message indicating the failure reason and shall not create a task entry for the failed file

### Requirement 9: File Operations

**User Story:** As a user, I want to rename, delete, create folders, and share files, so that I can organize and collaborate on my cloud files.

#### Acceptance Criteria

1. WHEN a user renames a file or folder, THE Graph_Client SHALL validate that the new name is between 1 and 400 characters and contains no invalid characters (\ / : * ? " < > |), update the file name via PATCH request, and refresh the file list upon success
2. WHEN a user confirms deletion of a file or folder, THE Graph_Client SHALL move it to the recycle bin (soft delete) via DELETE request and refresh the file list upon success
3. WHEN a user creates a folder, THE Graph_Client SHALL validate the folder name is between 1 and 400 characters and contains no invalid characters (\ / : * ? " < > |), then create the folder in the current directory with conflict behavior set to rename, and refresh the file list upon success
4. WHEN a user requests a share link, THE Graph_Client SHALL create an anonymous sharing link with configurable type (view or edit), optional expiration date, and optional password
5. WHEN a share link is successfully generated, THE App SHALL display the share link URL and provide a copy-to-clipboard action
6. WHEN a user requests file conversion on a supported file (Word, Excel, PowerPoint), THE Graph_Client SHALL convert the file to PDF format and prompt the user to select a local save location before saving
7. WHEN a user views file properties, THE App SHALL display file metadata including name, size, creation date, modification date, and web URL
8. IF a file operation (rename, delete, create folder, share, or convert) fails due to a network or API error, THEN THE App SHALL display an error message indicating the failure reason and preserve the current file list state unchanged

### Requirement 10: File Preview

**User Story:** As a user, I want to preview images and documents without downloading them, so that I can quickly check file contents.

#### Acceptance Criteria

1. WHEN a user opens an image file, THE Preview_Module SHALL display the image using a pre-authenticated preview URL from Graph API within 10 seconds of the user action
2. THE Preview_Module SHALL support the following image formats for preview: PNG, JPG, GIF, BMP, SVG, and WebP
3. WHEN a user opens a supported document (Word, Excel, PowerPoint, or PDF), THE Preview_Module SHALL render a preview using the Graph preview endpoint
4. THE Preview_Module SHALL support navigating to the next and previous previewable file in the current folder, following the current sort order of the file listing
5. IF the Preview_Module fails to load a preview due to an expired URL, unsupported file type, or network error, THEN THE Preview_Module SHALL display an error message indicating the failure reason and offer the option to retry or download the file instead

### Requirement 11: Search

**User Story:** As a user, I want to search for files by name, so that I can quickly find specific files across my drive.

#### Acceptance Criteria

1. WHEN a user submits a search query of at least 1 character, THE Graph_Client SHALL search within the current drive using the /search(q='{query}') endpoint and return up to 200 results
2. WHEN search results are received, THE File_Browser SHALL display each result with file name, parent folder path, size, and last modified date
3. WHEN the user selects local search scope, THE Graph_Client SHALL search only within the current folder and its subfolders using the folder's item ID as the search root; WHEN the user selects global search scope, THE Graph_Client SHALL search across the entire drive from the drive root
4. IF the search query returns no matching items, THEN THE File_Browser SHALL display an empty-state message indicating no results were found
5. WHEN a user double-clicks a search result, THE File_Browser SHALL navigate to the folder containing that item and highlight the selected file

### Requirement 12: Task Manager

**User Story:** As a user, I want to view and manage all my download and upload tasks, so that I can monitor transfer progress and control active operations.

#### Acceptance Criteria

1. THE Task_Manager SHALL display separate lists for download tasks and upload tasks, each showing the task file name and current state
2. THE Task_Manager SHALL show task status (downloading, uploading, paused, completed, failed), progress percentage (0–100), transfer speed in bytes per second, and elapsed time in seconds for each task
3. THE Task_Manager SHALL update displayed progress and speed at an interval no greater than 1 second
4. WHEN a user cancels a download task, THE Task_Manager SHALL abort the transfer, delete any partially downloaded files from disk, and remove the task from the download list
5. WHEN a user cancels an upload task, THE Task_Manager SHALL remove the task from the upload list
6. IF a download or upload task fails due to a network or API error, THEN THE Task_Manager SHALL set the task status to failed and display an error message indicating the failure reason
7. THE Task_Manager SHALL allow removing individual completed tasks or clearing all completed tasks at once from each list
8. WHEN a user clicks "open folder" on a completed download, THE App SHALL open the containing folder in the system file manager with the downloaded file selected

### Requirement 13: External Downloader Integration

**User Story:** As a user, I want to generate download configurations for Aria2, so that I can use a dedicated download manager for large files.

#### Acceptance Criteria

1. THE External_Downloader SHALL support sending download links to external downloaders via three modes: Aria2 (JSON-RPC), Motrix (JSON-RPC), and IDM (command-line invocation)
2. THE External_Downloader SHALL provide configurable RPC endpoint address and secret token fields for each RPC-based downloader, with default endpoint values of http://localhost:6800/jsonrpc for Aria2 and http://localhost:16800/jsonrpc for Motrix
3. WHEN a user submits a SharePoint sharing URL, THE External_Downloader SHALL parse URLs matching both sharepoint.com and sharepoint.cn domains to extract a direct download link in the format {domain}/personal/{user}/_layouts/52/download.aspx?share={shareId}
4. IF the submitted URL is not a valid SharePoint personal file sharing URL or is a folder sharing link, THEN THE External_Downloader SHALL not produce a direct link and SHALL not enable the push-to-downloader action
5. IF the RPC call to the external downloader fails, THEN THE External_Downloader SHALL display the error response to the user
6. WHEN a user triggers push-to-downloader with Aria2 or Motrix selected, THE External_Downloader SHALL send a JSON-RPC 2.0 request using the aria2.addUri method with the direct download link as the URI parameter, including the secret token prefixed with "token:" when a secret is configured

### Requirement 14: Internationalization

**User Story:** As a user, I want the app interface in my preferred language, so that I can use it comfortably.

#### Acceptance Criteria

1. THE i18n_Module SHALL provide complete translations for all user-visible strings in English (en-US) and Simplified Chinese (zh-CN) locales
2. WHEN the App starts and no language preference is saved in Settings_Store, THE i18n_Module SHALL detect the system locale and apply the matching language; IF the system locale does not match any supported locale, THEN THE i18n_Module SHALL default to English (en-US)
3. WHEN a user changes the language in settings, THE i18n_Module SHALL update all user-visible strings (labels, buttons, tooltips, and status messages) without requiring an app restart and SHALL persist the selection to Settings_Store
4. THE App SHALL store all user-visible strings in locale resource files, with no hardcoded text in the UI layer
5. WHEN the App starts and a language preference exists in Settings_Store, THE i18n_Module SHALL apply the saved language preference, overriding the system locale

### Requirement 15: Theme and Appearance

**User Story:** As a user, I want to switch between dark and light themes, so that I can use the app comfortably in different lighting conditions.

#### Acceptance Criteria

1. THE Theme_Engine SHALL support three mutually exclusive modes: light mode, dark mode, and system-default mode
2. WHILE the App is set to system-default mode, WHEN the operating system theme changes, THE Theme_Engine SHALL update the app theme to match the new system setting without requiring an app restart
3. WHEN a user selects a theme in settings, THE Settings_Store SHALL persist the choice and THE Theme_Engine SHALL apply it to all application UI surfaces without requiring an app restart or page navigation
4. THE Theme_Engine SHALL apply the same color scheme and component visual styles on both Windows and macOS platforms
5. IF the persisted theme preference is missing or unreadable, THEN THE Theme_Engine SHALL fall back to system-default mode

### Requirement 16: Storage Information Display

**User Story:** As a user, I want to see my cloud storage usage, so that I can monitor available space.

#### Acceptance Criteria

1. WHEN a user views drive details, THE App SHALL display total storage capacity, used space, and remaining space retrieved from the Graph API drive quota endpoint
2. THE App SHALL present storage information with a progress bar indicating the ratio of used space to total capacity, and human-readable size formatting that selects the most appropriate unit (KB, MB, GB, TB) based on magnitude (e.g., values under 1 MB displayed in KB, values under 1 GB displayed in MB)
3. IF the Graph API quota request fails or returns no quota data, THEN THE App SHALL display an error indication in place of the storage information

### Requirement 17: Application Update

**User Story:** As a user, I want to check for and install application updates, so that I can always use the latest version.

#### Acceptance Criteria

1. WHEN a user triggers an update check, THE App SHALL query the GitHub releases API for the latest version within 15 seconds
2. IF a new version is available, THEN THE App SHALL display the version number and changelog
3. WHEN a user confirms the update, THE App SHALL download the platform-specific archive matching the current OS and architecture (Windows x64, Windows ARM64, or macOS aarch64), replace the application files, and prompt the user to restart the App to complete the update
4. IF the update check fails due to network error or API unavailability, THEN THE App SHALL display an error message indicating the update check could not be completed and allow the user to retry
5. IF no new version is available, THEN THE App SHALL inform the user that the application is already up to date
6. IF the update download fails or is interrupted, THEN THE App SHALL discard any partially downloaded data, display an error message indicating the download failure, and preserve the current application version unchanged

### Requirement 18: Persistent Configuration

**User Story:** As a user, I want my settings and account data preserved between sessions, so that I do not need to reconfigure the app each time.

#### Acceptance Criteria

1. THE Settings_Store SHALL persist user preferences (theme, language, window position, and window dimensions) in a local JSON configuration file
2. THE Settings_Store SHALL persist connected account metadata (home account ID, drive ID, cloud type, display name) in a separate local JSON data file
3. WHEN the App starts and a valid configuration file exists, THE Settings_Store SHALL load previously saved configuration and restore the theme, language, and window geometry (position and dimensions)
4. IF the configuration file is missing or contains invalid JSON on startup, THEN THE Settings_Store SHALL apply default values (system theme, system locale, default window dimensions of 1280x720 centered) without displaying an error to the user
5. WHEN a user changes any preference in settings, THE Settings_Store SHALL write the updated configuration to disk within 1 second
6. THE Settings_Store SHALL use platform-appropriate application data directories (AppData\Roaming on Windows, ~/Library/Application Support on macOS) and SHALL create the directory if it does not exist

### Requirement 19: Navigation Structure

**User Story:** As a user, I want a clear sidebar navigation, so that I can quickly access different sections of the app.

#### Acceptance Criteria

1. THE App SHALL provide a persistently visible sidebar navigation with sections: Home, Files (drives), Task Manager, Tools, and Settings
2. WHEN a user selects the Files section, THE App SHALL present the navigation hierarchy: account list → service selection (OneDrive / SharePoint / Shared) → file browsing
3. THE App SHALL support tab-based file browsing allowing up to 10 drive tabs open simultaneously, where each tab represents an independent file browsing session
4. THE App SHALL visually distinguish the currently active navigation item in the sidebar using a distinct selected-state indicator
5. WHEN a user opens a drive for file browsing, THE App SHALL create a new tab if that drive is not already open, or switch to the existing tab if it is
6. WHEN a user closes a file browsing tab, THE App SHALL remove that tab and switch focus to the nearest remaining tab, or return to the Files section if no tabs remain
