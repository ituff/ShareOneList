---
inclusion: always
---

# 项目概述

ShareOneList（仓库名 ShareOneList）是 Microsoft 365 的跨平台 OneDrive / SharePoint 文件管理客户端，基于 Tauri 2 构建，支持 Windows（x64 / arm64）和 macOS（Apple Silicon），同时支持国际版和世纪互联（21Vianet）版 Microsoft 365。

- **仓库地址：** https://github.com/ituff/ShareOneList
- **框架：** Tauri 2.x，Rust 后端 + React 18 / TypeScript 前端
- **Graph API：** Rust `reqwest` 直连 Microsoft Graph REST API，不使用 Graph SDK
- **认证：** OAuth2 authorization code + PKCE，refresh token 存平台安全存储（keyring）
- **状态管理：** Zustand；**样式：** Tailwind CSS；**i18n：** react-i18next

# 架构

```
tauri-app/
├── src/                     # React 前端
│   ├── components/          # 页面和组件
│   ├── stores/              # Zustand 状态
│   ├── hooks/               # 主题、窗口状态、快捷键
│   ├── i18n/                # en-US / zh-CN
│   └── lib/                 # 类型、tauri invoke 封装、工具
└── src-tauri/src/           # Rust 后端
    ├── auth/                # OAuth2、会话、token 刷新
    ├── graph/               # Graph API 调用与校验
    ├── transfer/            # 下载 / 上传引擎（断点续传）
    ├── config/              # 配置持久化与旧版迁移
    ├── llm/                 # AI 助手：供应商配置（keyring）、适配器、SSE 流式聊天
    ├── store/               # SQLite：聊天历史（chat_history.db）、用户记忆
    ├── content/             # Office / PDF 文本提取（docx/pptx/xlsx/pdf）
    ├── catalog/             # Drive Catalog：跨 drive 目录地图（catalog.db + FTS5）
    └── tools/               # URL 解析、外部分发器、更新器

```

前端补充：`components/home/`（首页搜索 + 问 AI 入口、聊天视图）、`components/settings/LlmSettings.tsx`（模型供应商管理）、`stores/llmStore.ts`。

# AI 助手与 Drive Catalog

- **LLM 接入**：任意 OpenAI 兼容供应商（内置 OpenAI / Azure / DeepSeek / 百炼等预设）。供应商配置存 `llm.json`，API Key 只进 keyring（条目 `llm_api_key_*`），接口类型分 `openai_compatible` 与 `azure_openai` 两种适配器。聊天走 SSE 流式，delta 通过 `llm-chat-event` 推送；系统提示词由后端持有（`build_system_prompt`），前端传入的 system 消息会被剥离。
- **Grounding**：每次提问先查 Drive Catalog 选候选 drive（visit 加权前 4），对每个关键词单独执行 Graph 搜索（缓解中文长句低召回），小文本/Office/PDF 文件内容截断注入提示词，答案引用以可点击卡片呈现。云端与对话无依据时必须先询问用户再用公共知识。
- **Drive Catalog**（catalog.db，派生数据，可整体删除重建）：以 drive 为作用域单元。**渐进式积累，不做后台爬取**——注册时仅根层种子（一次 children 调用），目录知识来自浏览回写（FileBrowser 去抖）、搜索/grounding 命中（含祖先目录链）与 AI 读取三类回写；visit_count 只增，desc 只填空。FTS5 trigram 支持中文子串，<3 字符回退 LIKE，账号隔离为硬约束。
- **聊天历史**（chat_history.db）：会话与消息持久化， reasoning 思考过程与引用文件随消息保存；`PRAGMA user_version` 做前向迁移。两个 SQLite 文件均为派生数据，损坏即重建，禁止当作真相源。

# 双云架构

项目同时支持国际版和世纪互联版，核心差异集中在 `tauri-app/src-tauri/src/auth/cloud_config.rs`：

| 配置项 | 国际版 (Global) | 世纪互联版 (China) |
|--------|----------------|-------------------|
| Authority | login.microsoftonline.com/common | login.partner.microsoftonline.cn/organizations |
| Graph API | graph.microsoft.com/v1.0 | microsoftgraph.chinacloudapi.cn/v1.0 |
| SharePoint 域名 | sharepoint.com | sharepoint.cn |
| Scopes | 简写形式 (User.Read) | 完整 URI 前缀 |
| ClientId | 内置 Global ClientId | 内置 China ClientId |

两个版本的 Azure AD 完全独立，需要分别在 portal.azure.com 和 portal.azure.cn 注册应用。

# 世纪互联 API 限制

以下 Graph API 在世纪互联环境下不可用或行为不同：
- `/sites?search=*` — 不支持通配符搜索，报语法错误
- `/me/followedSites` — 依赖 OneDrive 许可证（MySite），无许可证报 "User's mysite not found"
- `/me/drive/sharedWithMe` — 同样依赖 OneDrive 许可证

当前 SharePoint 站点发现使用 M365 Groups 方式（`/me/memberOf` → 筛选 Unified Groups → `/groups/{id}/sites/root`）。

# 导航流程

文件页（账户列表）
  └─ 双击账户 → 服务选择（OneDrive / SharePoint / 与我共享）
       ├─ OneDrive → 文件浏览 Tab
       ├─ SharePoint → 站点列表 → 文档库列表 → 文件浏览 Tab
       └─ 与我共享 → 文档库列表 → 文件浏览 Tab

# 构建与运行

```bash
cd tauri-app
npm install
npm run tauri dev      # 开发模式
npm run build          # TypeScript 检查 + Vite 构建
cargo test --manifest-path src-tauri/Cargo.toml
```

# 多语言

所有用户可见文本走 `src/i18n/` 下的 en-US 和 zh-CN 资源，默认跟随系统语言，可在设置中手动切换。修改文案时必须同步更新两个语言文件。

# 编码规范

- Rust 后端拥有敏感逻辑与业务逻辑，前端通过 `src/lib/tauri.ts` 的类型化封装调用 IPC
- 文件 / 文件夹类型严格依据 Graph API 返回值（`folder` / `file` facet）判断，不依赖名称猜测
- 下载 / 上传进度通过 Tauri event 推送，前端订阅后更新 UI
- 修改 UI 文案必须同步更新 `en-US.json` 和 `zh-CN.json`
