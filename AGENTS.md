# AGENTS.md

本文件从 Kiro steering 和 Tauri 重写 spec 整理而来，用于快速接手 ShareOneList。Kiro 会自动加载仓库根目录的 `AGENTS.md`，因此它必须始终保持与 `.kiro/steering/`、`.kiro/specs/` 以及实际代码一致。

## 项目定位

ShareOneList 是 Microsoft 365 的跨平台 OneDrive / SharePoint 文件管理客户端，基于 Tauri 2 构建，支持 Windows（x64 / arm64）和 macOS（Apple Silicon），同时支持国际版和世纪互联（21Vianet）版。

当前仓库只保留 Tauri 2 主路径（`tauri-app/`），旧版 WinUI 3 代码已移除。所有任务默认在 `tauri-app/` 中工作。

## 权威文档

- `.kiro/steering/project-context.md`：项目概述、架构、双云差异、导航、构建和多语言约定。
- `.kiro/steering/git-rules.md`：git 操作红线，必须无条件遵守。
- `.kiro/specs/tauri-rewrite/requirements.md`：19 条功能需求及验收标准。
- `.kiro/specs/tauri-rewrite/design.md`：Tauri 架构、模块接口、数据模型、正确性属性和测试策略。
- `.kiro/specs/tauri-rewrite/tasks.md`：实现任务清单，当前任务均标记完成。

README 以 Tauri 2 主路径为准；遇到 README 与 spec 不一致时，以 `.kiro/specs/tauri-rewrite/` 和实际代码为准。

## 技术栈

| 层 | 技术 |
|---|---|
| 应用框架 | Tauri 2.x |
| 后端 | Rust，`reqwest` 直连 Microsoft Graph API，不使用 Graph SDK |
| 前端 | React 18 + TypeScript（strict） |
| 构建 | Vite |
| 状态 | Zustand |
| 样式 | Tailwind CSS + shadcn/ui |
| i18n | react-i18next，en-US 和 zh-CN |
| 安全存储 | Rust `keyring`，Windows Credential Manager / macOS Keychain |
| 配置 | Tauri `app_data_dir` 下的 JSON 文件 |
| IPC | `#[tauri::command]` + 前端 `invoke`，流式进度用 Tauri event |

## 仓库结构

```text
tauri-app/                         # Tauri 2 重写（主路径）
  src/
    components/                    # 页面和组件
    stores/                        # Zustand 状态
    hooks/                         # 主题、窗口状态、快捷键等
    i18n/                          # en-US.json / zh-CN.json
    lib/                           # types.ts、tauri.ts、formatters.ts、validators.ts
  src-tauri/src/
    auth/                          # OAuth2、会话、token 刷新
    graph/                         # Graph API 调用、校验
    transfer/                      # 下载 / 上传引擎
    config/                        # 配置持久化
    tools/                         # 外部分发器、URL 解析、更新
    models.rs / errors.rs          # 共享模型和错误类型
.kiro/steering/                    # Kiro steering
.kiro/specs/tauri-rewrite/         # 重写 spec
```

## 架构原则

- Rust 后端拥有所有敏感逻辑和业务逻辑：认证、token、Graph API、文件传输、配置持久化。前端只是表现层。
- 前端通过 `src/lib/tauri.ts` 中类型化的 invoke 封装调用后端；Rust 命令在对应模块的 `commands.rs` 中注册。新增命令时必须同时更新 Rust handler、`lib.rs` 的 `invoke_handler` 和 TypeScript 封装。
- 下载/上传进度通过 Tauri event 推送到前端，前端 Zustand store 订阅后更新 UI，不要改成轮询。
- Rust 模块按 `auth`、`graph`、`transfer`、`config`、`tools` 分层，不要让前端跨层直接访问 Graph。
- 前后端模型必须同步：Rust `models.rs` 与 `src/lib/types.ts` 保持同一套字段语义。

## 双云环境约束

| 项 | Global | China (21Vianet) |
|---|---|---|
| Authority | `login.microsoftonline.com/common` | `login.partner.microsoftonline.cn/organizations` |
| Graph Base URL | `https://graph.microsoft.com/v1.0` | `https://microsoftgraph.chinacloudapi.cn/v1.0` |
| SharePoint 域名 | `sharepoint.com` | `sharepoint.cn` |
| Scope | 简写形式 | 完整 URI 前缀 |
| Client ID | `AzureAD.Global.ClientId` | `AzureAD.China.ClientId` |

两个云环境必须保持独立会话：token 刷新、过期或登录失败不得互相影响。`CloudEnvironment` 是判断环境的核心，不要在 UI 层按字符串散落判断。

世纪互联环境下以下 Graph API 不可用或行为不同：

- `/sites?search=*` 不支持通配符搜索。
- `/me/followedSites` 依赖 OneDrive 许可证。
- `/me/drive/sharedWithMe` 依赖 OneDrive 许可证。

世纪互联 SharePoint 站点发现使用 M365 Groups 流程：`/me/memberOf` → Unified Groups → `/groups/{id}/sites/root`。

## 开发命令

Tauri 主路径：

```bash
cd tauri-app
npm install
npm run tauri dev     # 启动 Tauri 应用
npm run build         # TypeScript 检查 + Vite 构建
cargo test --manifest-path src-tauri/Cargo.toml
```

## 本地敏感信息

服务器地址、用户名、密码等敏感信息只放在仓库根目录的 `AGENTS.local.md` 中，该文件已被 `.gitignore` 忽略，禁止写入本文件。

## 编码约定

- 所有用户可见文本必须走 i18n：使用 `src/i18n/` 下的 en-US 和 zh-CN，禁止硬编码。
- TypeScript 开启 strict；共享类型放在 `src/lib/types.ts`。
- 文件名校验：1 到 400 字符，禁止 `\ / : * ? " < > |`。校验逻辑放 `graph/validators.rs` 或 `lib/validators.ts`，不要散落在组件里。
- 文件列表排序：文件夹在前、文件在后，同组内按名称字母排序。
- 错误处理采用分层策略：Rust 返回 `AppError`，Graph 5xx 最多重试 3 次并指数退避，token 剩余 5 分钟以内静默刷新，刷新失败才提示重新登录，配置损坏时回退默认值。前端用 toast、dialog、inline state 或任务状态呈现错误。
- 纯函数和边界逻辑按 design 文档的 15 条正确性属性补充测试；Rust property test 使用 `proptest`，tag 格式为 `// Feature: tauri-rewrite, Property {N}: {title}`。
- 修改 UI 文案时必须同时更新 `en-US.json` 和 `zh-CN.json`。

## Git 红线

- 禁止自动执行 `git commit`、`git push` 或任何同步到远程仓库的操作。
- 所有 commit / push 操作必须在执行前明确征得用户同意。
- `git status`、`git diff`、`git log` 等只读命令可以自由使用。

## 维护规则

- 架构或约定变化时，先更新 `.kiro/steering/`，再同步本文件。
- 新增功能按 Kiro spec 流程维护：`.kiro/specs/<feature>/` 下保留 requirements、design、tasks。
- 删除或重命名模块前，先检查 `src/lib/tauri.ts`、`src-tauri/src/lib.rs` 和 spec 中的接口引用。
