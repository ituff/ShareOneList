# ShareOneList 开发计划

## 概述

ShareOneList 是一个跨平台云文件管理工具，基于 Tauri 2 + React + Rust 构建，核心能力是连接 Microsoft Graph API，管理 OneDrive 和 SharePoint 上的文件。

当前已完成：多账号登录、双云环境（Global / 世纪互联）、文件浏览、上传下载、搜索预览、任务管理、Teams 会议录像清单（v2.1.0）、AI 聊天助手（v2.2.0-beta.1：OpenAI 兼容供应商接入、云端文件 grounding 与引用卡片、Office/PDF 内容提取、思考过程展示、聊天历史持久化）以及 Drive Catalog 渐进式检索层（catalog.db + FTS5，跨站点/多文档库定位）。

## 下一阶段目标

### 1. 用户聊天记忆

按 `.kiro/specs/ai-memory/` 实施：从对话中沉淀用户级持久事实与偏好，注入后续对话系统提示词；设置页管理 + `#remember` / `#forget` 指令。存储于 chat_history.db（迁移 v2）。

### 2. WebDAV 挂载

在 Rust 后端启动 loopback HTTP 服务，将 Graph API 映射为 WebDAV 协议，Windows 资源管理器和 macOS Finder 可直接挂载。先做只读 PoC，验证中文名、大文件、连接稳定性，再做写入和缓存优化。

### 3. 文件夹比较 + 同步

会话式双栏比较工具，支持跨账号、跨云环境（Global ↔ 世纪互联）对比文件夹差异。文件级差异判断优先使用 Graph 哈希，目录树可复用 Drive Catalog 的积累数据，差异结果可导出报告或生成同步计划。先做只读比较，再叠加动作执行。

### 4. 移动版（远期）

桌面端功能稳定后，基于 Tauri 2 适配 iOS/Android，包括 OAuth deep link、安全存储、沙盒文件系统、响应式 UI。

## 技术底座

多个功能共享：Graph 元数据缓存与限流策略、SQLite 本地存储（已落地：chat_history.db 聊天历史/记忆、catalog.db 目录地图）、统一的传输 stream 层、后台任务与进度事件模型。
