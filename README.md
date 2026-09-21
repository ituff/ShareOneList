# ShareOneList

English | [简体中文](./README_zh_CN.md)

> Cross-platform Microsoft 365 file manager, built with Tauri 2, Rust and React.

ShareOneList is a file management tool focused on Microsoft 365. One app covers **Global (International) organization accounts, Global personal accounts, and China (21Vianet) accounts** on Windows (x64 / arm64) and macOS (Apple Silicon) — browse OneDrive and SharePoint, and easily download and manage **Teams meeting recordings**.

## Highlights

- **Three account types, one app** — Global (International) organizations, Global personal (Microsoft account), and China (21Vianet) are all first-class citizens. Each account type gets the service entries that fit it: OneDrive, SharePoint, and Teams meeting recordings.
- **Teams meeting recordings** — Discover recordings across your OneDrive, SharePoint sites, and Microsoft Search, play them in the built-in player, and download them in one click. Recordings locked by download policies can still be saved with the built-in streaming extractor (see the [wiki](https://github.com/ituff/ShareOneList/wiki)).
- **Cross-platform** — Windows (x64 / arm64) and macOS (Apple Silicon) builds from the same codebase
- **OneDrive + SharePoint** — Browse your personal OneDrive, SharePoint site libraries, and shared drives in one place
- **Multi-account** — Add multiple accounts across cloud environments, with custom aliases and icons
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

The macOS dmg bundles a fix script (`fix-macos-gatekeeper.command`) and bilingual first-launch instructions — open the mounted dmg and double-click the script (right-click → Open if macOS asks). If that doesn't apply to your setup, follow the steps below.

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
- [x] Global (International) organization & personal accounts, and 21Vianet (China) support
- [x] Teams meeting recordings: aggregate, play, and download (streaming extractor for download-restricted recordings)
- [x] Multi-account management with custom aliases and icons
- [x] Batch download merged into one task
- [x] Resumable download with progress and speed
- [x] Download to a user-selected save path with last-path memory
- [x] File sharing & link generation
- [x] File preview (Image, Video, Markdown, Office Online)
- [x] Image / video thumbnails
- [x] Sortable file list (name / size / modified, ascending or descending)
- [x] Explorer-style breadcrumbs that collapse long paths
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
