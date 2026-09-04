# ShareOneList 开发计划

## 概述

ShareOneList 是一个跨平台云文件管理工具，基于 Tauri 2 + React + Rust 构建，核心能力是连接 Microsoft Graph API，管理 OneDrive 和 SharePoint 上的文件。

当前已完成基础功能：多账号登录、双云环境（Global / 世纪互联）、文件浏览、上传下载、搜索预览、任务管理。

## 下一阶段目标

### 1. AI 聊天助手

在首页加入 AI 聊天窗口。用户选择文件或文件夹作为上下文，Graph 搜索定位相关文件，读取内容后交给模型问答。不做本地 RAG 和向量索引，答案带引用卡片，可跳转到文件预览。

### 2. WebDAV 挂载

在 Rust 后端启动 loopback HTTP 服务，将 Graph API 映射为 WebDAV 协议，Windows 资源管理器和 macOS Finder 可直接挂载。先做只读 PoC，验证中文名、大文件、连接稳定性，再做写入和缓存优化。

### 3. 文件夹比较 + 同步

会话式双栏比较工具，支持跨账号、跨云环境（Global ↔ 世纪互联）对比文件夹差异。文件级差异判断优先使用 Graph 哈希，懒加载目录树，差异结果可导出报告或生成同步计划。先做只读比较，再叠加动作执行。

### 4. 会议录像清单

将 OneDrive 和 SharePoint 中的 Teams 会议录像聚合为统一列表，支持按时间、来源、所有者筛选。自己的录像可预览和下载，分享的录像根据权限展示下载或只读状态。

### 5. 移动版（远期）

桌面端功能稳定后，基于 Tauri 2 适配 iOS/Android，包括 OAuth deep link、安全存储、沙盒文件系统、响应式 UI。

## 技术底座

多个功能共享：Graph 元数据缓存与限流策略、SQLite 本地存储、统一的传输 stream 层、后台任务与进度事件模型。
