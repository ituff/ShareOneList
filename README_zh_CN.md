<div align="center">

# ShareOneList

**把你的 Microsoft 365 文件管明白的桌面应用**

[![Release](https://img.shields.io/github/v/release/ituff/ShareOneList)](https://github.com/ituff/ShareOneList/releases)
[![Downloads](https://img.shields.io/github/downloads/ituff/ShareOneList/total)](https://github.com/ituff/ShareOneList/releases)
![Platform](https://img.shields.io/badge/platform-Windows%20x64%20%7C%20arm64%20%7C%20macOS%20Apple%20Silicon-blue)

[English](./README.md) | 简体中文

[📥 下载安装](#-下载安装) · [✨ 功能总览](#-核心功能) · [💬 反馈问题](https://github.com/ituff/ShareOneList/issues)

</div>

---

ShareOneList 是一款专注 Microsoft 365 的跨平台桌面文件管理器。**国际版组织账户、国际版个人账户、世纪互联（21Vianet）账户**三种 Microsoft 365 在同一个应用里并行使用；浏览 OneDrive 与 SharePoint 就像操作本地资源管理器；内置 **AI 助手**让云盘"开口回答"——提问时自动检索你的文件，基于文件内容作答；**Teams 会议录像**聚合播放、一键下载。

基于 Tauri 2 + Rust + React 构建：安装包小、启动快、内存占用低，配置与凭据全部保存在本机。

## ✨ 核心功能

### 🤖 AI 助手：直接问你的云盘

- 提问自动检索 OneDrive / SharePoint，读取 **docx / pptx / xlsx / pdf** 等文档内容后作答，回答附带**可点击的引用卡片**，来源可溯
- 接入任意 OpenAI 兼容供应商：内置 OpenAI、Azure OpenAI、DeepSeek、阿里百炼、Moonshot、智谱、Ollama 预设，支持连接测试和在线拉取模型列表
- 推理模型的思考过程实时展示，结束后自动折叠为一行，随时展开回看
- 聊天历史本地保存：重启不丢会话，多轮上下文完整保留
- **AI 记忆**：用 `#remember` / `#forget` 沉淀你的偏好与习惯，后续对话自动生效，回答中会披露引用了哪些记忆

### ☁️ 三种 Microsoft 365，一个应用

- 国际版组织账户、国际版个人账户、世纪互联账户都是一等公民，各自提供合适的服务入口
- 多账户同时登录、并行浏览，可自定义别名和图标
- 世纪互联全流程适配：独立 OAuth 端点、Graph 端点、SharePoint 站点发现，与国际版会话完全隔离互不影响

### 🎬 Teams 会议录像

- 聚合散落在 OneDrive、SharePoint 站点和 Microsoft Search 中的会议录像
- 内置播放器直接播放，一键下载；下载被策略限制的录像可用**流式提取**保存为 MP4（详见 [Wiki](https://github.com/ituff/ShareOneList/wiki)）

### 🗺️ Drive Catalog 目录地图

- 跨站点、跨文档库的渐进式位置索引：随着浏览、搜索和使用 AI，它越来越知道你的文件在哪
- 让全局搜索和 AI 回答能直接命中 SharePoint 文档库深处的文件

### 🚀 为大文件而生的下载引擎

- 断点续传：下载中断、重启应用后都能接着下
- 批量下载：一次选中多个文件，归并为一个任务，进度与速度一目了然
- 国内加速：下载失败自动回退多个加速镜像，流式写入磁盘 + 实时进度

### 🔄 配置备份与恢复

- 一键导出 / 导入配置备份：包含全部设置与账户信息（不含登录凭据），换机、重装一步恢复
- 自动备份：指定 OneDrive 同步文件夹后，设置或账户变更自动写入最新备份，由 OneDrive 同步上云

### 还有这些

- 🔍 全局搜索：跨账户搜索，按来源账户、文件类型、修改日期筛选
- 👁 在线预览：图片、视频、Markdown、Office 文档，支持图片 / 视频缩略图
- 🗂 文件管理：书签、分享链接、重命名、删除、属性、转 PDF、存储容量显示
- 🖼 详细 / 缩略图 / 看图三种布局，拖拽上传，列头点击排序，长路径面包屑自动折叠
- 🔔 通知中心 + 应用内检查更新，正式版 / 测试版双更新通道
- 🌗 深色 / 浅色主题；English / 简体中文 / 日本語 / Deutsch 四种界面语言

## 📥 下载安装

从 [Releases](https://github.com/ituff/ShareOneList/releases) 下载对应平台的安装包：

| 平台 | 安装包 |
|---|---|
| Windows 10/11 x64 | `.exe` 安装包 / `.msi` |
| Windows 10/11 arm64 | `.exe` 安装包 / `.msi` / 绿色版 `.zip` |
| macOS (Apple Silicon) | `.dmg` |

> 想第一时间尝鲜新功能？在 **设置 → 关于** 中把更新渠道切换为"测试版"，即可收到预发布版本推送。

<details>
<summary><b>macOS 首次打开提示"无法验证开发者"？</b></summary>

dmg 内已附带修复脚本 `fix-macos-gatekeeper.command` 和中英文说明——挂载 dmg 后双击脚本即可（若仍提示，右键 → 打开）。也可以手动执行：

```bash
xattr -cr /Applications/ShareOneList.app
open /Applications/ShareOneList.app
```

如果提示权限不足，请使用 `sudo xattr -cr /Applications/ShareOneList.app`。辅助脚本见 [scripts/fix-macos-gatekeeper.command](./scripts/fix-macos-gatekeeper.command)。

</details>

## 🚀 快速上手

1. 安装并启动 ShareOneList
2. 进入 **文件** 页，点击 **添加网盘**，登录你的 Microsoft 账户（可添加多个不同环境的账户）
3. 双击网盘开始浏览；也可以在首页直接 **搜索** 或 **问 AI**

## 📸 截图

| 首页 | 云盘 |
|---|---|
| ![](./ScreenShots/HomePage.png) | ![](./ScreenShots/CloudPage.png) |
| **网盘中心** | **文件浏览** |
| ![](./ScreenShots/DriveHubPage.png) | ![](./ScreenShots/DrivePage.png) |
| **网格布局** | **任务管理** |
| ![](./ScreenShots/GridLayout.png) | ![](./ScreenShots/TaskManager.png) |
| **书签** | **工具** |
| ![](./ScreenShots/BookmarksPage.png) | ![](./ScreenShots/ToolsPage.png) |
| **设置** | **深色模式** |
| ![](./ScreenShots/SettingsPage.png) | ![](./ScreenShots/DarkMode.png) |

## ⚙️ 高级配置

应用内置国际版和世纪互联版的 Azure AD 客户端 ID，开箱即用。如需使用自己的 Azure AD 应用，请在 [portal.azure.com](https://portal.azure.com)（国际版）和 [portal.azure.cn](https://portal.azure.cn)（世纪互联版）分别注册，并在应用内配置。

## 🛠️ 从源码构建

```bash
git clone https://github.com/ituff/ShareOneList.git
cd ShareOneList/tauri-app
npm install
npm run tauri dev
```

技术栈与架构约定见 [AGENTS.md](./AGENTS.md)，开发计划见 [DEV_PLAN.md](./DEV_PLAN.md)，功能规格见 [.kiro/specs/](./.kiro/specs/)。
