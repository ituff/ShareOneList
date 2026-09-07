# 用户聊天记忆 — 设计文档

## 架构总览

复用现有 `store/`（SQLite）与 `llm/`（模型调用）模块，新增记忆存储与提取逻辑；前端在设置页加管理区块、聊天输入处理 `#remember` / `#forget` 指令。

```text
tauri-app/src-tauri/src/store/
  memory.rs       # memories 表（chat_history.db 迁移 v2）+ CRUD
tauri-app/src-tauri/src/llm/
  memory.rs       # 提取/合并 prompt、JSON 解析、非流式调用
  client.rs       # build_system_prompt 增加 memories 段（签名扩展）
  commands.rs     # llm_chat 注入 + 提取调度；memory_* 命令

前端：
  src/components/settings/MemorySettings.tsx
  ChatView：#remember / #forget 指令解析 + "已记住"确认气泡
```

## 数据模型（chat_history.db，PRAGMA user_version → 2）

```sql
CREATE TABLE memories (
  id TEXT PRIMARY KEY,             -- 'mem_' + uuid
  content TEXT NOT NULL,           -- 记忆内容（用户语言原样）
  source_conversation_id TEXT NOT NULL DEFAULT '',  -- 来源会话（手动创建为空）
  enabled INTEGER NOT NULL DEFAULT 1,
  pinned INTEGER NOT NULL DEFAULT 0,
  use_count INTEGER NOT NULL DEFAULT 0,
  last_used_at INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
```

配置（`llm.json` 的 `LlmConfig` 扩展，serde default 保持向后兼容）：

```rust
pub struct LlmMemoryConfig {
    pub enabled: bool,          // default true
    pub extract_every: u32,     // 每 N 条用户消息触发一次提取，default 6
}
// LlmConfig { ..., #[serde(default)] memory: LlmMemoryConfig }
```

## 记忆提取（llm/memory.rs）

触发（后端）：`chat_append_message` 持久化 **user** 消息后，统计该会话自上次提取以来的 user 消息数（会话内存计数 + 表内可重建），达到 `extract_every` 且 `memory.enabled` → spawn 后台任务（不阻塞命令返回）。

提取调用：非流式 chat（复用 `test_connection` 的请求形态，max_tokens=800）：

- 输入：现有启用记忆列表 + 最近 ≤10 条消息（从 messages 表读）。
- system prompt（后端持有）：

```text
You maintain long-term memory for a desktop AI assistant. Given the existing
memories and a recent conversation window, output the updated memory list.
Rules:
- Keep only durable facts about the user: work context, preferences,
  recurring file locations, explicit instructions. Drop chit-chat.
- Merge duplicates keeping the richer phrasing; on conflict prefer the newer.
- At most 20 items, each one sentence, same language as the conversation.
Respond with strict JSON only: {"memories": ["..."]}
```

- 解析：剥 ```json 围栏 → serde 解析 → 失败则丢弃本次（下次重试）。单测覆盖围栏/前后噪声/非法 JSON。
- 写回：事务内 `DELETE FROM memories WHERE source != 'manual'`（pinned 保留）+ 插入新列表（手动与置顶项不动）。为保 use_count/last_used 语义，合并时按内容精确匹配旧条目继承统计；不匹配视为新条目。

调度保护：同一时间仅一个提取任务运行（`MemoryExtractMutex`）；会话切换/删除不打断已在跑的任务（读的是消息快照）。

## 记忆注入（llm_chat）

`llm_chat` 在构建系统提示词前：

1. `memory.enabled` 为假 → 跳过。
2. 读取 enabled 记忆：`ORDER BY pinned DESC, last_used_at DESC LIMIT 20`，累计字符 ≤2000 截断。
3. `build_system_prompt(context_files, location_hints, memories)` 追加段：

```text
Known user context (from memory, reference naturally — not exhaustive,
do not recite that you remember):
- …
```

4. 注入后刷新被选中条目的 `last_used_at` / `use_count`（尽力而为）。

## 显式指令（前端 ChatView）

- `#remember <内容>`：本地拦截（不发给模型），调 `memory_save(content, conversation_id)`，气泡内系统样式确认；空内容提示用法。
- `#forget <关键词>`：调 `memory_search_and_delete(keyword)` 返回删除数，确认气泡显示数量；0 条时提示无匹配。

指令只匹配行首（trim 后），正常聊天含 `#` 不受影响。

## 命令清单

| 命令 | 说明 |
|---|---|
| `memory_list` | 全部记忆（管理 UI） |
| `memory_save` | 新增/编辑（id 为空则新建） |
| `memory_delete` | 硬删除 |
| `memory_set_enabled` / `memory_set_pinned` | 状态切换 |
| `memory_search_and_delete` | `#forget` 用，返回删除数 |
| `memory_extract_now` | 手动触发一次提取（调试/设置页） |

设置页区块（`MemorySettings.tsx`，挂在 LlmSettings 之后）：总开关、提取阈值（3/6/12）、记忆列表（编辑/启停/置顶/删除）、手动新增。

## 正确性属性（测试锚点）

1. Property: 注入预算不变量 —— 任意记忆集合注入后条数 ≤20 且字符 ≤2000（proptest 随机内容长度）。
2. Property: 禁用隔离 —— enabled=0 或功能关闭时，`build_system_prompt` 输出不包含任何记忆内容。
3. Property: 删除硬性 —— `memory_delete` 后 `memory_list` 与注入集合均不含该 id。
4. Property: JSON 解析鲁棒 —— 围栏/前后噪声可解析，非法输入返回 None 而非 panic。
5. Property: 提取合并保序 —— 手动（manual 来源）与 pinned 条目在任何自动合并后原样保留。
6. Property: 统计继承 —— 内容未变的记忆跨合并保留 use_count/last_used_at。

## 风险与开放问题

- **提取质量**：小模型 JSON 输出不稳定 → prompt 约束 + 解析容错 + 失败静默重试；必要时设置里换"更强模型"再触发。
- **隐私边界**：记忆会随提示词外发至模型服务商 —— 设置页开关旁明示；文档与隐私提示同步。
- **记忆膨胀**：上限 20 条由提取 prompt 控制，手动条目超限时注入端截断（pinned 优先）。
