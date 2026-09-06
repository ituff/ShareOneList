# AI 聊天助手 — 任务清单（第一版）

- [x] 1. Rust：`llm/config.rs` — LlmConfig 类型、llm.json 持久化、keyring 存取
  - [x] 1.1 LlmProviderConfig / LlmModelConfig / LlmConfig 模型与默认值
  - [x] 1.2 LlmConfigManager：load/save（损坏回退默认）
  - [x] 1.3 API key 写入/读取掩码/删除（keyring service `shareonelist`）
  - [x] 1.4 校验：default 引用存在性、删除默认供应商拒绝
- [x] 2. Rust：`llm/client.rs` — 适配器与流式客户端
  - [x] 2.1 ProviderAdapter trait + openai_compatible / azure_openai 实现
  - [x] 2.2 test_connection：最小请求与错误分类
  - [x] 2.3 SSE 流式解析 + Tauri event 推送 + CancellationToken 取消
- [x] 3. Rust：`llm/commands.rs` + lib.rs 注册 + 状态管理
- [x] 4. TS：types.ts 模型同步 + tauri.ts 封装
- [x] 5. 前端：`stores/llmStore.ts` 与 `components/settings/LlmSettings.tsx`
  - [x] 5.1 供应商列表 / 编辑对话框 / 删除 / 设默认
  - [x] 5.2 测试连接与错误呈现
- [x] 6. 前端：`components/home/HomePage.tsx`
  - [x] 6.1 搜索 + 聊天双入口与空状态引导
  - [x] 6.2 多账号全局搜索结果与预览跳转
  - [x] 6.3 聊天视图：流式渲染、模型选择器、停止、重试
- [x] 7. i18n：en-US / zh-CN 同步
- [x] 8. 测试：config round-trip、默认引用不变量、删除默认拒绝、SSE 解析
- [x] 9. 验证：cargo test + npm run build
- [x] 10. 后续打磨：错误提示可读化、流式竞态修复、reasoning_content 思考过程（可折叠）、markdown 渲染、模型列表模糊匹配下拉、入口问题自动提交
- [x] 11. 输入栏重构：M365 账户多选（localStorage 记忆上次选择）、模型/思考强度（reasoning_effort）内嵌下拉、圆形发送钮
- [x] 12. 导航：侧边栏新增「问 AI」「搜索」tab（紧跟首页），首页入口跳转至对应 tab 并携带输入内容
- [x] 13. 文件上下文注入：搜索命中的小文本文件（≤200KB，扩展名白名单）读取内容摘要（最多 3 个 / 每个 4000 字符）注入系统提示词；用户气泡下方渲染可点击的引用卡片，跳转文件预览
- [x] 14. Office/PDF 内容解析：Rust `content/` 模块（docx/pptx 走 zip+XML 抽取、xlsx 抽 sharedStrings、pdf 走 pdf-extract）+ `extract_file_text` 命令（≤10MB）；前端按扩展名路由（docx/pptx/xlsx/pdf 上限 5MB）
- [x] 15. 搜索页筛选：来源（M365 账户）/ 文件类型（文件夹/文档/表格/演示/图片/其他）/ 修改日期（今天/7天/30天/一年），纯前端过滤，无匹配结果独立提示
- [x] 16. 聊天历史持久化（SQLite）：Rust `store/chat_history`（rusqlite bundled、conversations/messages 两表、PRAGMA user_version 迁移）+ chat_* 五个命令；前端自动加载最近会话、每轮问答自动落库（用户消息含引用文件）、历史下拉切换/删除、新对话按钮

## 后续阶段（未排期）

- [ ] drive catalog 检索层（多站点/多库、访问回写、FTS5）
