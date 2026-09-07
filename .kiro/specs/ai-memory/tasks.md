# 用户聊天记忆 — 任务清单

- [x] 1. Rust `store/memory.rs`：chat_history.db 迁移 v2（memories 表）+ CRUD
  - [x] 1.1 建表迁移（幂等，与 v1 共存）
  - [x] 1.2 list / save / delete / set_enabled / set_pinned / search_and_delete
  - [x] 1.3 Property 3（删除硬性）、Property 6（统计继承由合并逻辑保证）相关单测
- [x] 2. Rust `llm/memory.rs`：提取与合并
  - [x] 2.1 提取 system prompt + 非流式调用（默认模型，max_tokens 800）
  - [x] 2.2 JSON 解析容错（围栏/噪声/非法）—— Property 4 测试
  - [x] 2.3 合并写回（manual/pinned 保留，内容匹配继承统计）—— Property 5 测试
  - [x] 2.4 单飞保护（MemoryExtractMutex）+ 失败静默
- [x] 3. `LlmConfig` 扩展 memory 配置（enabled / extract_every，serde default）
- [x] 4. 注入：`build_system_prompt` 加 memories 段 + `llm_chat` 读取/截断/刷新统计
  - [x] 4.1 Property 1（预算不变量）、Property 2（禁用隔离）测试
- [x] 5. 提取调度：`chat_append_message`（user 消息）计数触发 + `memory_extract_now` 命令
- [x] 6. 命令注册 + types.ts / tauri.ts 封装
- [x] 7. 前端 ChatView：`#remember` / `#forget` 指令解析 + 确认气泡
- [x] 8. 前端设置页 `MemorySettings.tsx`：开关、阈值、列表管理（编辑/启停/置顶/删除/新增）
- [x] 9. i18n（en-US / zh-CN 同步）+ 隐私文案更新（记忆会随提示词发送）
- [x] 10. 验证：cargo test + npm run build + spec 同步

## 验收清单（人工）

- [x] 连续聊 6+ 条 → 设置页出现自动提取的记忆
- [x] 新对话提问 → 回答体现旧会话沉淀的偏好/背景
- [x] `#remember 我常用 DeepSeek` → 立即出现确认与条目
- [x] `#forget DeepSeek` → 删除对应条目并确认数量
- [x] 关闭总开关 → 注入停止；删除 chat_history.db 中记忆后不再注入（会话消息保留）
