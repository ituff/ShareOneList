<div align="center">

# ShareOneList

**The desktop app that gets your Microsoft 365 files under control**

[![Release](https://img.shields.io/github/v/release/ituff/ShareOneList)](https://github.com/ituff/ShareOneList/releases)
[![Downloads](https://img.shields.io/github/downloads/ituff/ShareOneList/total)](https://github.com/ituff/ShareOneList/releases)
![Platform](https://img.shields.io/badge/platform-Windows%20x64%20%7C%20arm64%20%7C%20macOS%20Apple%20Silicon-blue)

English | [简体中文](./README_zh_CN.md)

[📥 Download](#-download) · [✨ Features](#-features) · [💬 Issues](https://github.com/ituff/ShareOneList/issues)

</div>

---

ShareOneList is a cross-platform desktop file manager built for Microsoft 365. **Global organization accounts, Global personal accounts, and China (21Vianet) accounts** all live side by side in one app; browse OneDrive and SharePoint like a native file explorer; the built-in **AI assistant** answers questions about *your* files — it searches your cloud first, then answers based on what it finds; and **Teams meeting recordings** are aggregated for instant playback and one-click download.

Built with Tauri 2 + Rust + React: small installer, fast startup, low memory footprint. Your configuration and credentials never leave your machine.

## ✨ Features

### 🤖 AI assistant: ask your cloud directly

- Questions automatically search OneDrive / SharePoint, read **docx / pptx / xlsx / pdf** content, and answer with **clickable citation cards** — every claim traceable to a file
- Works with any OpenAI-compatible provider: built-in presets for OpenAI, Azure OpenAI, DeepSeek, Alibaba Bailian, Moonshot, Zhipu, and Ollama, with connection testing and online model listing
- Reasoning models show their thinking live, then collapse into a single line you can expand anytime
- Chat history is stored locally: conversations survive restarts with full multi-turn context
- **AI memory**: capture your preferences with `#remember` / `#forget`; they apply to future chats and are disclosed inside answers

### 🎬 Teams meeting recordings

- Aggregates recordings scattered across OneDrive, SharePoint sites, and Microsoft Search
- Play in the built-in player, download in one click; recordings locked by download policy can still be saved as MP4 with the **streaming extractor** (see the [wiki](https://github.com/ituff/ShareOneList/wiki))

### ☁️ Three flavors of Microsoft 365, one app

- Global organizations, Global personal (Microsoft account), and China (21Vianet) accounts are all first-class citizens, each with the service entries that fit it
- Sign in to multiple accounts across cloud environments and browse in parallel, with custom aliases and icons
- Full 21Vianet adaptation — dedicated OAuth endpoints, Graph endpoints, and SharePoint site discovery — with sessions fully isolated from the Global cloud

### 🗺️ Drive Catalog

- A progressive location index across sites and document libraries: the more you browse, search, and ask, the better it knows where your files live
- Lets global search and AI answers hit files buried deep in SharePoint document libraries

### 🚀 A download engine built for big files

- Resumable downloads: pick up after interruptions and app restarts
- Batch downloads: select many files, get one task with clear progress and speed
- Mirror acceleration: automatic fallback across download mirrors, streamed to disk with live progress

### 🔄 Config backup & restore

- One-click export / import of a backup with all settings and accounts (no login credentials) — migration and reinstall in one step
- Auto backup: point it at a OneDrive-synced folder and every setting or account change writes the latest backup, synced to the cloud by OneDrive

### And more

- 🔍 Global search across accounts, filterable by account, file type, and modified date
- 👁 Online preview for images, videos, Markdown, and Office documents, with thumbnails
- 🗂 Bookmarks, share links, rename, delete, properties, convert to PDF, storage usage
- 🖼 Details / grid / gallery layouts, drag-and-drop upload, sortable columns, collapsing breadcrumbs
- 🔔 Notification center and in-app update checks with stable / beta update channels
- 🌗 Dark / light theme; English, 简体中文, 日本語, and Deutsch UI

## 📥 Download

Grab the installer for your platform from [Releases](https://github.com/ituff/ShareOneList/releases):

| Platform | Installer |
|---|---|
| Windows 10/11 x64 | `.exe` setup / `.msi` |
| Windows 10/11 arm64 | `.exe` setup / `.msi` / portable `.zip` |
| macOS (Apple Silicon) | `.dmg` |

> Want new features as soon as they ship? Switch the update channel to **Beta** in **Settings → About** to receive pre-release versions.

<details>
<summary><b>macOS says the app "can't be verified"?</b></summary>

The dmg bundles a fix script (`fix-macos-gatekeeper.command`) with bilingual instructions — mount the dmg and double-click the script (if macOS still complains, right-click → Open). Or run manually:

```bash
xattr -cr /Applications/ShareOneList.app
open /Applications/ShareOneList.app
```

If the command reports insufficient permissions, use `sudo xattr -cr /Applications/ShareOneList.app`. The helper script is at [scripts/fix-macos-gatekeeper.command](./scripts/fix-macos-gatekeeper.command).

</details>

## 🚀 Getting started

1. Install and launch ShareOneList
2. Open **Files**, click **Add drive**, and sign in with your Microsoft account (add as many accounts as you like, across cloud environments)
3. Double-click a drive to browse — or use **Search** / **Ask AI** straight from the home page

## 📸 Screenshots

| Home | Cloud |
|---|---|
| ![](./ScreenShots/HomePage.png) | ![](./ScreenShots/CloudPage.png) |
| **Drive Hub** | **File browsing** |
| ![](./ScreenShots/DriveHubPage.png) | ![](./ScreenShots/DrivePage.png) |
| **Grid layout** | **Task manager** |
| ![](./ScreenShots/GridLayout.png) | ![](./ScreenShots/TaskManager.png) |
| **Bookmarks** | **Tools** |
| ![](./ScreenShots/BookmarksPage.png) | ![](./ScreenShots/ToolsPage.png) |
| **Settings** | **Dark mode** |
| ![](./ScreenShots/SettingsPage.png) | ![](./ScreenShots/DarkMode.png) |

## ⚙️ Advanced configuration

The app ships with default Azure AD client IDs for both Global and 21Vianet — no setup needed. To use your own Azure AD applications, register them at [portal.azure.com](https://portal.azure.com) (Global) and [portal.azure.cn](https://portal.azure.cn) (21Vianet) separately and configure the client IDs in the app.

## 🛠️ Build from source

```bash
git clone https://github.com/ituff/ShareOneList.git
cd ShareOneList/tauri-app
npm install
npm run tauri dev
```

Questions or feedback? Open an [issue](https://github.com/ituff/ShareOneList/issues).
