# ShareOneList

English | [简体中文](./README_zh_CN.md)

> Cross-platform OneDrive & SharePoint file manager for Microsoft 365, built with Tauri 2, Rust and React.

ShareOneList is a cross-platform file management client for Microsoft 365, supporting Windows (x64 / arm64) and macOS (Apple Silicon). It provides unified access to OneDrive and SharePoint document libraries, with full support for both the Global (International) and China (21Vianet) environments.

## Highlights

- **Dual-cloud support** — Manage files on both Global and 21Vianet Microsoft 365 from a single app
- **OneDrive + SharePoint** — Browse your personal OneDrive, SharePoint site libraries, and shared drives in one place
- **Multi-account** — Add multiple accounts across different cloud environments
- **Resumable downloads** — Pause and resume interrupted download tasks after restart
- **Batch tasks** — Merge a batch download into one task with progress and speed
- **File preview** — Preview images, videos, Markdown, and Office documents online
- **Thumbnails** — File-type icons and image/video thumbnails
- **Bookmarks** — Save frequently used folders and files for quick access
- **Dark / light theme** — Follows the system theme or switch manually
- **Internationalization** — Follows the system language, manually switchable between English and Simplified Chinese

## Getting Started

1. Download the latest release from [Releases](https://github.com/ituff/ShareOneList/releases)
2. Install or run `ShareOneList`
3. Click **Files** in the sidebar, then **Add drive** to sign in with your Microsoft account
4. Double-click a drive to browse files

### macOS Gatekeeper

The current macOS builds are unsigned and not notarized, so the first launch may show "app is damaged" or "cannot be opened". Remove the quarantine attribute before opening:

```bash
curl -fsSL https://raw.githubusercontent.com/ituff/ShareOneList/main/scripts/fix-macos-gatekeeper.command | bash
```

Or run it manually after moving the app to `/Applications`:

```bash
xattr -cr /Applications/ShareOneList.app
open /Applications/ShareOneList.app
```

If the command reports insufficient permissions, use `sudo xattr -cr /Applications/ShareOneList.app`. Alternatively, right-click the app and choose **Open** to confirm once. The helper script is available at [scripts/fix-macos-gatekeeper.command](./scripts/fix-macos-gatekeeper.command).

## Configuration

The app ships with default Azure AD Client IDs for both Global and 21Vianet. If you want to use your own Azure AD applications, register them at [portal.azure.com](https://portal.azure.com) (Global) and [portal.azure.cn](https://portal.azure.cn) (21Vianet) separately and configure the client IDs in the app.

## Features

- [x] OneDrive file browsing
- [x] SharePoint site & document library browsing
- [x] Global (International) and 21Vianet (China) support
- [x] Multi-account management
- [x] Batch download merged into one task
- [x] Resumable download with progress and speed
- [x] Download to a user-selected save path with last-path memory
- [x] File sharing & link generation
- [x] File preview (Image, Video, Markdown, Office Online)
- [x] Image / video thumbnails
- [x] Bookmarks
- [x] Rename / Delete / Properties
- [x] Convert to PDF
- [x] Storage capacity display
- [x] List / Grid / Gallery layout modes
- [x] Drag-and-drop upload
- [x] Dark / light theme
- [x] System language / English / 简体中文
- [x] In-app update check
- [ ] Mobile release

## Screenshots

![HomePage](./ScreenShots/HomePage.png)
![CloudPage](./ScreenShots/CloudPage.png)
![DriveHubPage](./ScreenShots/DriveHubPage.png)
![DrivePage](./ScreenShots/DrivePage.png)
![GridLayout](./ScreenShots/GridLayout.png)
![TaskManager](./ScreenShots/TaskManager.png)
![BookmarksPage](./ScreenShots/BookmarksPage.png)
![ToolsPage](./ScreenShots/ToolsPage.png)
![SettingsPage](./ScreenShots/SettingsPage.png)
![DarkMode](./ScreenShots/DarkMode.png)

## Development

```bash
cd tauri-app
npm install
npm run tauri dev
```
