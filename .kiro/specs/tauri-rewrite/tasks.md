# Implementation Plan: Tauri Rewrite

## Overview

Rewrite ShareOneList from WinUI 3 (.NET/C#) to Tauri 2.x (Rust backend + React/TypeScript frontend) targeting Windows and macOS. The implementation proceeds in layers: project scaffolding → core backend modules → frontend shell → feature integration → testing → wiring and polish.

## Tasks

- [x] 1. Project scaffolding and core interfaces
  - [x] 1.1 Initialize Tauri 2.x project with Rust backend and React + TypeScript + Vite frontend
    - Run `create-tauri-app` or equivalent to scaffold the project structure
    - Configure `Cargo.toml` with dependencies: `reqwest`, `serde`, `serde_json`, `keyring`, `tokio`, `chrono`, `uuid`, `tauri`
    - Configure `package.json` with dependencies: `react`, `react-dom`, `zustand`, `react-i18next`, `i18next`, `tailwindcss`, `@tauri-apps/api`
    - Set up Tailwind CSS and shadcn/ui component library
    - Create directory structure: `src-tauri/src/{auth, graph, transfer, config, tools}` and `src/{components, stores, hooks, i18n, lib}`
    - _Requirements: 1.1, 1.2_

  - [x] 1.2 Define core Rust data types and error enums
    - Create `src-tauri/src/models.rs` with all shared data types: `DriveItem`, `Drive`, `DriveQuota`, `Site`, `ParentReference`, `ProgressEvent`, `AppConfig`, `ThemeMode`, `WindowState`, `AccountEntry`, `ShareOptions`, `ExternalDownloaderConfig`, `DownloaderType`, `UpdateInfo`, `CloudConfig`
    - Create `src-tauri/src/errors.rs` with `AppError` enum (Network, Auth, GraphApi, FileSystem, Config, Transfer, Validation variants) and implement `Serialize` and `Into<tauri::InvokeError>`
    - Create `src-tauri/src/auth/cloud_config.rs` with `CloudEnvironment` enum and `CloudEnvironment::config()` method
    - _Requirements: 3.1, 3.2, 3.3_

  - [x] 1.3 Define frontend TypeScript types mirroring Rust models
    - Create `src/lib/types.ts` with TypeScript interfaces: `DriveItem`, `Drive`, `DriveQuota`, `Site`, `BreadcrumbItem`, `TabState`, `AppConfig`, `AccountEntry`, `ShareOptions`, `ProgressEvent`, `ExternalDownloaderConfig`, `UpdateInfo`
    - Create `src/lib/tauri.ts` with typed Tauri invoke wrapper functions for all IPC commands
    - _Requirements: 1.2_

  - [x]* 1.4 Write property tests for cloud environment configuration
    - **Property 3: Cloud environment configuration correctness**
    - **Validates: Requirements 2.2, 2.7, 3.2, 3.3**

- [x] 2. Configuration and persistent storage
  - [x] 2.1 Implement ConfigManager in Rust
    - Create `src-tauri/src/config/mod.rs` with `ConfigManager` struct
    - Implement `load_config()` that reads JSON from platform app data dir, returns `AppConfig`
    - Implement `save_config()` that serializes `AppConfig` to JSON and writes to disk
    - Implement `load_accounts()` and `save_accounts()` for `Vec<AccountEntry>`
    - Use Tauri's `app_data_dir` API for platform-appropriate paths (AppData\Roaming on Windows, ~/Library/Application Support on macOS)
    - Create directory if it does not exist on first write
    - Implement fallback to defaults (system theme, system locale, 1280×720 centered) for missing or invalid config
    - _Requirements: 18.1, 18.2, 18.3, 18.4, 18.5, 18.6_

  - [x] 2.2 Register Tauri commands for config operations
    - Create `get_config` command returning `AppConfig`
    - Create `save_config` command accepting `AppConfig`
    - Create `get_accounts` command returning `Vec<AccountEntry>`
    - Wire commands in `main.rs` with `tauri::generate_handler!`
    - _Requirements: 18.1, 18.3_

  - [x]* 2.3 Write property tests for AppConfig serialization round-trip
    - **Property 1: AppConfig serialization round-trip**
    - **Validates: Requirements 1.5, 18.1, 18.3**

  - [x]* 2.4 Write property tests for AccountEntry serialization round-trip
    - **Property 2: AccountEntry serialization round-trip**
    - **Validates: Requirements 4.3, 18.2**

  - [x]* 2.5 Write property test for invalid config fallback
    - **Property 4: Invalid or missing configuration falls back to defaults**
    - **Validates: Requirements 18.4, 15.5**

- [x] 3. Authentication module
  - [x] 3.1 Implement OAuth2 authorization code flow with PKCE in Rust
    - Create `src-tauri/src/auth/mod.rs` with `AuthModule` struct holding `HashMap<CloudEnvironment, AuthSession>`
    - Implement `login()`: open system browser for OAuth2 authorization, listen on localhost redirect URI, exchange code for tokens
    - Implement `logout()`: clear tokens from keyring, remove session
    - Implement `get_token()`: return current access token, refreshing if needed
    - Implement `refresh_token_if_needed()`: check expiry, call token endpoint with refresh token if ≤5 min remaining
    - Use `keyring` crate for secure storage of refresh tokens (Windows Credential Manager / macOS Keychain)
    - Support both Global and China authority endpoints
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8, 2.9_

  - [x] 3.2 Register Tauri auth commands
    - Create `login` command accepting `cloud_env: String` → returns `AccountInfo`
    - Create `logout` command accepting `cloud_env: String`
    - Manage concurrent sessions (one per CloudEnvironment) with independence guarantee
    - _Requirements: 2.8, 3.4, 3.7_

  - [x]* 3.3 Write property test for token refresh timing decision
    - **Property 9: Token refresh timing decision**
    - **Validates: Requirements 2.5**

- [x] 4. Checkpoint - Core infrastructure
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Graph API client
  - [x] 5.1 Implement GraphClient base with environment-aware endpoints
    - Create `src-tauri/src/graph/mod.rs` with `GraphClient` struct holding `reqwest::Client` and `CloudEnvironment`
    - Implement `base_url()` returning correct Graph API base URL per environment
    - Implement retry logic: up to 3 retries with exponential backoff (1s initial, 2x multiplier, 30s max) for 5xx errors and network timeouts
    - _Requirements: 3.2, 3.3_

  - [-] 5.2 Implement drive and file listing operations
    - Implement `list_children()`: GET `/drives/{driveId}/items/{itemId}/children` with pagination ($top=200, @odata.nextLink)
    - Implement `get_drive()`: GET `/drives/{driveId}`
    - Implement `get_quota()`: GET `/drives/{driveId}` extracting quota object
    - Implement `search()`: GET `/drives/{driveId}/root/search(q='{query}')` with scope support (local = item/{id}/search, global = root/search)
    - _Requirements: 5.1, 5.8, 11.1, 11.3, 16.1_

  - [-] 5.3 Implement file mutation operations
    - Implement `rename_item()`: PATCH `/drives/{driveId}/items/{itemId}` with `{name: newName}`
    - Implement `delete_item()`: DELETE `/drives/{driveId}/items/{itemId}`
    - Implement `create_folder()`: POST `/drives/{driveId}/items/{parentId}/children` with folder facet and conflict behavior rename
    - Implement `create_share_link()`: POST `/drives/{driveId}/items/{itemId}/createLink` with type, expiration, password options
    - Implement `convert_format()`: GET `/drives/{driveId}/items/{itemId}/content?format={format}` for PDF conversion
    - Implement `get_preview_url()`: POST `/drives/{driveId}/items/{itemId}/preview`
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7, 10.1, 10.3_

  - [-] 5.4 Implement SharePoint operations
    - Implement `search_sites()`: GET `/sites?search=*` for Global environment
    - Implement `get_sites_via_groups()`: GET `/me/memberOf` → filter Unified Groups → GET `/groups/{id}/sites/root` for China environment
    - Implement `get_site_drives()`: GET `/sites/{siteId}/drives`
    - Implement `get_shared_drives()`: GET `/me/drive/sharedWithMe`
    - _Requirements: 6.1, 6.2, 6.3, 6.5_

  - [x] 5.5 Register Tauri commands for Graph API operations
    - Register all file, drive, SharePoint, search, and property commands as Tauri commands
    - Each command fetches token from AuthModule, constructs GraphClient, calls method, returns typed result
    - Handle errors by converting to `AppError` and returning as invoke error
    - _Requirements: 5.1, 6.1, 9.1, 11.1_

  - [ ]* 5.6 Write property test for file name validation
    - **Property 5: File name validation**
    - **Validates: Requirements 9.1, 9.3**

- [x] 6. Download engine
  - [x] 6.1 Implement multi-chunk download engine in Rust
    - Create `src-tauri/src/transfer/download.rs` with `DownloadEngine` struct
    - Implement `create_task()`: get download URL from Graph API, calculate 8 chunks (each ≤1 MB), spawn async download
    - Implement parallel chunk downloading using `tokio::spawn` with concurrency limit of 8
    - Implement `pause_task()`: signal chunks to stop, record progress
    - Implement `resume_task()`: check URL freshness (>1 hour → request new URL), resume from last byte position
    - Implement `cancel_task()`: abort task, delete partial files
    - Emit `ProgressEvent` via Tauri events at least once per second with speed calculation
    - Handle file name conflicts by appending suffix
    - _Requirements: 7.1, 7.2, 7.4, 7.5, 7.6, 7.9, 7.10_

  - [x] 6.2 Implement folder download with recursive traversal
    - Implement `download_folder()`: list folder contents recursively, create local directory structure, enqueue all files
    - _Requirements: 7.3_

  - [x] 6.3 Register Tauri download commands and event emissions
    - Register `download_file`, `download_folder`, `pause_download`, `resume_download`, `cancel_download`, `open_containing_folder` commands
    - Implement `open_containing_folder` using platform-specific shell commands (explorer.exe /select, or open -R on macOS)
    - _Requirements: 7.1, 7.5, 12.8_

  - [ ]* 6.4 Write property test for download URL freshness check
    - **Property 10: Download URL freshness check**
    - **Validates: Requirements 7.6**

  - [ ]* 6.5 Write property test for download chunk calculation
    - **Property 12: Download chunk calculation**
    - **Validates: Requirements 7.2**

- [x] 7. Upload engine
  - [x] 7.1 Implement upload engine with strategy selection in Rust
    - Create `src-tauri/src/transfer/upload.rs` with `UploadEngine` struct
    - Implement simple upload (PUT) for files ≤4 MB
    - Implement upload session (POST createUploadSession → PUT chunks of 320 KB) for files >4 MB
    - Retry failed chunks up to 3 times with exponential backoff
    - Emit `ProgressEvent` via Tauri events at least once per second
    - Use conflict behavior "rename" for name collisions
    - _Requirements: 8.1, 8.2, 8.6, 8.7, 8.8_

  - [x] 7.2 Implement folder upload with recursive structure creation
    - Implement `upload_folder()`: traverse local directory, create cloud folders, upload all files
    - _Requirements: 8.3_

  - [x] 7.3 Register Tauri upload commands
    - Register `upload_files`, `upload_folder`, `cancel_upload` commands
    - _Requirements: 8.1_

  - [ ]* 7.4 Write property test for upload strategy selection
    - **Property 11: Upload strategy selection by file size**
    - **Validates: Requirements 8.1**

- [x] 8. Checkpoint - Backend core complete
  - Ensure all tests pass, ask the user if questions arise.

- [x] 9. Frontend shell and navigation
  - [x] 9.1 Implement main application layout with sidebar navigation
    - Create `src/components/layout/Sidebar.tsx` with navigation items: Home, Files, Task Manager, Tools, Settings
    - Create `src/components/layout/MainContent.tsx` as router for content area
    - Create `src/App.tsx` with sidebar + content layout
    - Implement active navigation state with visual distinction
    - _Requirements: 19.1, 19.4_

  - [x] 9.2 Implement window management and state restoration
    - Configure Tauri window settings (min size 800×600, default 1024×768)
    - Implement window state save on resize/move (debounced write to config)
    - Implement window state restore on startup from saved config
    - Handle saved position outside display bounds: reposition to center of primary display
    - _Requirements: 1.3, 1.4, 1.5, 1.6, 1.7_

  - [x] 9.3 Implement settings store with Zustand
    - Create `src/stores/settingsStore.ts` managing theme, language, window state
    - Load initial config from backend via `get_config` command on app startup
    - Persist changes to backend via `save_config` command on setting change
    - _Requirements: 18.1, 18.3, 18.5_

- [x] 10. Theme engine and internationalization
  - [x] 10.1 Implement theme engine with system detection
    - Create `src/hooks/useTheme.ts` managing light/dark/system modes
    - Apply Tailwind dark mode class based on selected theme
    - Listen for OS theme changes when in system mode using `window.matchMedia`
    - Persist theme preference to settings store
    - Ensure consistent appearance on both Windows and macOS
    - _Requirements: 15.1, 15.2, 15.3, 15.4, 15.5_

  - [x] 10.2 Implement i18n with react-i18next
    - Create `src/i18n/en-US.json` and `src/i18n/zh-CN.json` with all user-visible strings
    - Configure i18next with language detection (system locale → fallback to en-US)
    - Implement language switching without app restart
    - Persist language preference to settings store
    - _Requirements: 14.1, 14.2, 14.3, 14.4, 14.5_

  - [x]* 10.3 Write property test for locale detection and fallback
    - **Property 15: Locale detection and fallback**
    - **Validates: Requirements 14.2**

- [x] 11. Account management UI
  - [x] 11.1 Implement account list and login flow
    - Create `src/components/accounts/AccountList.tsx` displaying connected accounts with display name and cloud environment label
    - Create `src/components/accounts/LoginDialog.tsx` with cloud environment selector (Global / China)
    - Create `src/stores/authStore.ts` with Zustand for account state management
    - Implement add account flow: select environment → invoke login command → persist account
    - Implement remove account flow: confirm dialog → invoke logout → remove from store
    - Implement duplicate detection: reject if home_account_id already exists
    - Load cached accounts from backend on startup
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6, 4.7, 3.5_

  - [x]* 11.2 Write property test for account duplicate detection
    - **Property 13: Account duplicate detection**
    - **Validates: Requirements 4.5**

- [x] 12. File browser
  - [x] 12.1 Implement file browser with list/grid/gallery views
    - Create `src/components/files/FileBrowser.tsx` as the main file listing component
    - Create `src/components/files/FileItem.tsx` for individual file/folder row or card
    - Create `src/stores/fileStore.ts` with Zustand for file browsing state (items, loading, currentFolder, breadcrumbs)
    - Implement three layout modes: list view, grid view, gallery view
    - Display file name, human-readable size, last modified date, file type icon
    - Sort folders before files, alphabetical within each group (case-insensitive)
    - Implement pagination support (load more when >200 items)
    - Show empty-state indicator for empty folders
    - Show error state with retry for failed API calls
    - _Requirements: 5.1, 5.2, 5.3, 5.7, 5.8, 5.9, 5.10_

  - [x] 12.2 Implement breadcrumb navigation and keyboard shortcuts
    - Create `src/components/files/Breadcrumb.tsx` showing path hierarchy from drive root
    - Implement breadcrumb click to navigate to any ancestor folder
    - Implement Backspace key to navigate to parent folder (when not at root)
    - Implement double-click folder to navigate into it
    - _Requirements: 5.4, 5.5, 5.6_

  - [x] 12.3 Implement tab-based file browsing
    - Create `src/components/layout/TabBar.tsx` for drive tabs
    - Support up to 10 simultaneous tabs, each with independent browsing state
    - Implement tab creation when opening a new drive (or switch to existing tab if drive already open)
    - Implement tab close: switch to nearest tab, or return to Files section if no tabs remain
    - Store per-tab state: driveId, currentFolderId, breadcrumbs, items, layoutMode
    - _Requirements: 19.3, 19.5, 19.6_

  - [ ]* 12.4 Write property test for file listing sort order
    - **Property 8: File listing sort order invariant**
    - **Validates: Requirements 5.8**

  - [ ]* 12.5 Write property test for tab management invariants
    - **Property 14: Tab management invariants**
    - **Validates: Requirements 19.3, 19.5**

- [x] 13. Checkpoint - File browsing functional
  - Ensure all tests pass, ask the user if questions arise.

- [x] 14. File operations UI
  - [x] 14.1 Implement rename, delete, create folder, and share dialogs
    - Create rename dialog with validation (1–400 chars, no invalid characters)
    - Create delete confirmation dialog (soft delete to recycle bin)
    - Create new folder dialog with name validation
    - Create share link dialog with type selector (view/edit), optional expiration, optional password
    - Display share link with copy-to-clipboard button on success
    - Show error toast on any operation failure
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.8_

  - [x] 14.2 Implement file conversion and properties
    - Implement PDF conversion for Word/Excel/PowerPoint: invoke convert command → prompt save location → save file
    - Implement file properties view: display name, size, creation date, modification date, web URL
    - _Requirements: 9.6, 9.7_

  - [x] 14.3 Implement file preview
    - Create `src/components/files/FilePreview.tsx` modal for image and document preview
    - Support image formats: PNG, JPG, GIF, BMP, SVG, WebP via pre-authenticated preview URL
    - Support document preview (Word, Excel, PowerPoint, PDF) via Graph preview endpoint
    - Implement next/previous navigation between previewable files in current folder
    - Show error state with retry and download fallback option
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5_

- [x] 15. Search
  - [x] 15.1 Implement search UI and backend integration
    - Create search input in file browser toolbar
    - Implement search scope toggle: local (current folder subtree) vs global (entire drive)
    - Display results with file name, parent path, size, and last modified date
    - Implement double-click on search result to navigate to containing folder
    - Show empty-state message for no results
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5_

- [x] 16. Task manager UI
  - [x] 16.1 Implement download and upload task list display
    - Create `src/components/tasks/TaskManager.tsx` with separate download and upload task sections
    - Create `src/components/tasks/TaskItem.tsx` showing file name, status, progress bar, speed, elapsed time
    - Create `src/stores/taskStore.ts` with Zustand listening to Tauri progress events
    - Subscribe to `progress-event` Tauri events and update task state reactively
    - Update progress display at ≤1 second interval
    - _Requirements: 12.1, 12.2, 12.3_

  - [x] 16.2 Implement task control actions
    - Implement pause/resume buttons for download tasks
    - Implement cancel button for download and upload tasks
    - Implement "open folder" for completed downloads
    - Implement clear completed tasks (individual and batch)
    - Show error state with failure reason for failed tasks
    - _Requirements: 12.4, 12.5, 12.6, 12.7, 12.8_

  - [x] 16.3 Implement drag-and-drop upload and batch download
    - Implement file drag-and-drop onto file browser to trigger upload
    - Implement multi-select checkbox mode for batch downloads
    - Implement shutdown-after-download option
    - _Requirements: 7.7, 7.8, 8.4, 8.5_

- [x] 17. SharePoint navigation UI
  - [x] 17.1 Implement SharePoint site and library browsing
    - Create `src/components/files/SharePointSites.tsx` listing available sites
    - Create `src/components/files/DriveList.tsx` for document libraries within a site
    - Implement navigation flow: account → service selection (OneDrive/SharePoint/Shared) → sites → libraries → file browser
    - Handle environment-specific site discovery (Global: search, China: groups)
    - Show empty-state for no sites found
    - Show error state with retry for failed retrieval
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.6, 6.7_

- [x] 18. Tools and external downloader
  - [x] 18.1 Implement external downloader integration
    - Create `src-tauri/src/tools/url_parser.rs` for SharePoint URL parsing (sharepoint.com and sharepoint.cn domains)
    - Create `src-tauri/src/tools/downloader.rs` implementing JSON-RPC 2.0 calls to Aria2/Motrix and IDM command-line invocation
    - Create tools page UI with SharePoint URL input field, downloader type selector (Aria2/Motrix/IDM), RPC endpoint and secret configuration
    - Default endpoints: Aria2 = http://localhost:6800/jsonrpc, Motrix = http://localhost:16800/jsonrpc
    - Register `push_to_downloader` and `parse_sharepoint_url` Tauri commands
    - Show error response on RPC failure
    - _Requirements: 13.1, 13.2, 13.3, 13.4, 13.5, 13.6_

  - [ ]* 18.2 Write property test for SharePoint URL parsing
    - **Property 7: SharePoint sharing URL parsing**
    - **Validates: Requirements 13.3, 13.4**

- [x] 19. Storage information and application update
  - [x] 19.1 Implement storage quota display
    - Create storage info component showing total, used, remaining space with progress bar
    - Use human-readable formatting (KB/MB/GB/TB based on magnitude)
    - Show error indication if quota request fails
    - _Requirements: 16.1, 16.2, 16.3_

  - [x] 19.2 Implement application update check and install
    - Create `src-tauri/src/tools/updater.rs` querying GitHub releases API for latest version
    - Register `check_update` and `perform_update` Tauri commands
    - Create update UI: check button, version/changelog display, confirm and download flow
    - Download platform-specific archive (Windows x64/ARM64, macOS aarch64)
    - Handle errors: network failure with retry, download failure with discard
    - Show "up to date" message when no update available
    - _Requirements: 17.1, 17.2, 17.3, 17.4, 17.5, 17.6_

  - [ ]* 19.3 Write property test for file size formatting
    - **Property 6: Human-readable file size formatting**
    - **Validates: Requirements 5.3, 16.2**

- [x] 20. Checkpoint - All features integrated
  - Ensure all tests pass, ask the user if questions arise.

- [x] 21. Utility functions and formatters
  - [x] 21.1 Implement shared utility functions
    - Create `src/lib/formatters.ts` with file size formatter (bytes → KB/MB/GB/TB) and date formatter
    - Create `src/lib/validators.ts` with file name validation (1–400 chars, no invalid chars)
    - Create `src-tauri/src/graph/validators.rs` with Rust-side file name validation for pre-flight checks
    - _Requirements: 5.3, 9.1, 9.3, 16.2_

- [x] 22. Cross-platform build configuration
  - [x] 22.1 Configure Tauri build targets for Windows and macOS
    - Configure `tauri.conf.json` for Windows (x64, ARM64) and macOS (aarch64) builds
    - Set up platform-specific window configuration (native title bar, system controls)
    - Configure app icons and metadata for both platforms
    - Set up GitHub Actions CI/CD for multi-platform builds (replacing existing .NET workflows)
    - _Requirements: 1.1, 1.3_

- [x] 23. Final checkpoint - Full integration and polish
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document (using `proptest` crate for Rust)
- Unit tests validate specific examples and edge cases
- The frontend uses TypeScript with strict mode for type safety
- All Tauri commands use typed payloads serialized via serde
- The backend handles all sensitive operations (auth, tokens, API calls) — the frontend is a presentation layer only

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1"] },
    { "id": 1, "tasks": ["1.2", "1.3"] },
    { "id": 2, "tasks": ["1.4", "2.1", "9.1"] },
    { "id": 3, "tasks": ["2.2", "2.3", "2.4", "2.5", "9.2", "9.3"] },
    { "id": 4, "tasks": ["3.1", "10.1", "10.2"] },
    { "id": 5, "tasks": ["3.2", "3.3", "10.3", "21.1"] },
    { "id": 6, "tasks": ["5.1", "11.1"] },
    { "id": 7, "tasks": ["5.2", "5.3", "5.4", "11.2"] },
    { "id": 8, "tasks": ["5.5", "5.6", "6.1"] },
    { "id": 9, "tasks": ["6.2", "6.3", "6.4", "6.5", "7.1"] },
    { "id": 10, "tasks": ["7.2", "7.3", "7.4"] },
    { "id": 11, "tasks": ["12.1", "12.2"] },
    { "id": 12, "tasks": ["12.3", "12.4", "12.5", "14.1"] },
    { "id": 13, "tasks": ["14.2", "14.3", "15.1"] },
    { "id": 14, "tasks": ["16.1", "17.1"] },
    { "id": 15, "tasks": ["16.2", "16.3", "18.1"] },
    { "id": 16, "tasks": ["18.2", "19.1", "19.2"] },
    { "id": 17, "tasks": ["19.3", "22.1"] }
  ]
}
```
