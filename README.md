# ShareOneList

English | [简体中文](./README_zh_CN.md)

> OneDrive & SharePoint, Global & China — one app for all your clouds.

ShareOneList is a file management client for Microsoft 365 built with WinUI 3. It provides unified access to OneDrive and SharePoint document libraries, with full support for both the Global (International) and China (21Vianet) environments.

## Highlights

- **Dual-cloud support** — Manage files on both Global and 21Vianet Microsoft 365 from a single app
- **OneDrive + SharePoint** — Browse your personal OneDrive, SharePoint site libraries, and shared drives in one place
- **Multi-account** — Add multiple accounts across different cloud environments
- **Batch download** — Select files and folders with checkboxes, download them all at once
- **Task manager** — Track download and upload progress in real time
- **File preview** — Preview images, Markdown, and media files without leaving the app
- **File operations** — Upload, download, rename, delete, share, and convert files
- **Dark mode** — Follows system theme with Mica / Acrylic material support
- **Internationalization** — English and Simplified Chinese

## Getting Started

1. Download the latest release from [Releases](https://github.com/ituff/SimpleList21V/releases)
2. Unzip and run `ShareOneList.exe`
3. Click **Files** in the sidebar, then **Add drive** to sign in with your Microsoft account
4. Double-click a drive to browse files

## Configuration

Edit `appsettings.json` to set your own Azure AD Client IDs:

```json
{
  "AzureAD": {
    "Global": {
      "ClientId": "your-global-client-id"
    },
    "China": {
      "ClientId": "your-china-client-id"
    }
  }
}
```

> **Note:** Global and 21Vianet Azure AD are completely independent. Register your app at [portal.azure.com](https://portal.azure.com) (Global) and [portal.azure.cn](https://portal.azure.cn) (21Vianet) separately.

## Features

- [x] OneDrive file browsing
- [x] SharePoint site & document library browsing
- [x] Global (International) and 21Vianet (China) support
- [x] Multi-account management
- [x] Batch download with checkbox selection
- [x] Download / upload with progress tracking
- [x] File sharing & link generation
- [x] File preview (Image, Markdown, Media)
- [x] Rename / Delete / Properties
- [x] Convert to PDF
- [x] Storage capacity display
- [x] Column / Grid / Image layout modes
- [x] Drag-and-drop upload
- [x] Dark mode & theme customization
- [x] English / 简体中文
- [ ] Automatic synchronization

## Screenshots

> May not reflect the latest version.

![HomePage](./ScreenShots/HomePage.png)
![CloudPage](./ScreenShots/CloudPage.png)
![DrivePage](./ScreenShots/DrivePage.png)
![CreateFolder](./ScreenShots/CreateFolder.png)
![GridLayout](./ScreenShots/GridLayout.png)
![Download](./ScreenShots/Download.png)
![Share](./ScreenShots/Share.png)
![ImageViewing](./ScreenShots/ImageViewing.png)
![ToolsPage](./ScreenShots/ToolsPage.png)
![DarkMode](./ScreenShots/DarkMode.png)

## Acknowledgements

ShareOneList is forked from [aiguoli/SimpleList](https://github.com/aiguoli/SimpleList). Thanks to the original author for the open-source contribution.
