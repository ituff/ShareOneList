# Drive Catalog 检索层 — 设计文档

## 架构总览

延续现有分层：Rust 拥有全部业务逻辑，前端只调用命令与订阅事件。

```text
tauri-app/src-tauri/src/catalog/
  mod.rs        # 模块声明
  store.rs      # SQLite：schema、迁移、drives/nodes/usage CRUD + FTS5
  seed.rs       # 根层种子索引（注册时 1 次 children 调用）+ 手动深度索引（可选）
  query.rs      # FTS5 trigram 查询 + LIKE 回退 + 排序
  writeback.rs  # 访问回写：浏览 / 搜索命中 / AI 读取 三类来源
  commands.rs   # #[tauri::command] 入口

数据库：app_data_dir/catalog.db（独立于 chat_history.db；派生数据，可删）
事件：catalog-event { driveId, account, status, visitedNodes, currentPath }
```

核心转变（相对"预先 BFS 5 层"方案）：**indexer 从主角退为可选工具**。目录知识的主要来源是三类用户行为的回写——浏览积累、搜索命中积累、AI 读取积累。catalog 随使用逐渐逼近用户真实工作集的完整目录。

## 数据模型（SQLite，PRAGMA user_version 迁移，当前 v1）

```sql
CREATE TABLE drives (
  account_id  TEXT NOT NULL,      -- home_account_id
  cloud_env   TEXT NOT NULL,      -- 'global' | 'china'
  drive_id    TEXT NOT NULL,
  kind        TEXT NOT NULL,      -- 'onedrive' | 'documentLibrary'
  name        TEXT NOT NULL,
  site_name   TEXT NOT NULL DEFAULT '',
  status      TEXT NOT NULL DEFAULT 'queued',  -- queued|seeding|ready|failed
  node_count  INTEGER NOT NULL DEFAULT 0,
  last_used_at INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (account_id, drive_id)
);

CREATE TABLE nodes (
  account_id  TEXT NOT NULL,
  drive_id    TEXT NOT NULL,
  item_id     TEXT NOT NULL,      -- 祖先链节点可为 ''
  path        TEXT NOT NULL,      -- 相对 drive 根，'/' 分隔，根为 ''
  name        TEXT NOT NULL,
  kind        TEXT NOT NULL,      -- 'folder' | 'file'
  desc        TEXT NOT NULL DEFAULT '',
  parent_path TEXT NOT NULL DEFAULT '',
  last_visited INTEGER NOT NULL DEFAULT 0,
  visit_count INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (account_id, drive_id, path)
);
CREATE INDEX idx_nodes_parent ON nodes(account_id, drive_id, parent_path);

-- FTS5：trigram 分词保证 CJK 子串命中；触发器与 nodes 同步
CREATE VIRTUAL TABLE node_fts USING fts5(name, path, desc, tokenize='trigram');

CREATE TABLE usage (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  account_id TEXT NOT NULL,
  source TEXT NOT NULL,           -- 'browse' | 'search' | 'grounding' | 'manual'
  question TEXT NOT NULL DEFAULT '',
  payload TEXT NOT NULL DEFAULT '[]'
);
```

要点：

- 删除 `depth` 列与 `indexed_at`（不再有深度概念；根层种子通过 `status: seeding→ready` 表达）。
- 祖先链节点 `item_id` 允许为空（搜索命中只提供路径，不提供中间目录 id；用户浏览到该目录时由浏览回写补全 item_id）。
- **desc 来源**：浏览/命中回写时由前端提供一句话（如命中原因、目录用途），或 AI 读取文件内容时由内容主题回填；建图侧不再启发式生成。
- trigram 索引约为文本 2-3 倍体积；只存元数据，可接受。

## 三条积累路径（writeback.rs 为主）

### 1. 浏览回写（最高频，覆盖面最大）

`FileBrowser` 的 `list_files` 成功后，前端 fire-and-forget 调用：

```text
catalog_record_browse(account, drive_id, cloud_env, folder_path, items: DriveItem[])
```

- 文件夹节点：`visit_count + 1`、`last_visited=now`、子项 upsert（item_id 齐全）。
- 该文件夹若已在库 → 刷新；不在库 → 插入。子项天然同步（重命名/删除/新增随浏览刷新，满足需求 6.1）。
- 批量一个事务；失败静默。

### 2. 搜索命中回写

grounding / 全局搜索返回结果后：

```text
catalog_record_hits(account, drive_id, source, question, hits: [{path, item_id, name, kind}])
```

- 文件节点 upsert + `visit_count + 1`。
- **祖先链**：按 `/` 切 path，逐级 upsert folder 节点（item_id=''，count +1）——不枚举兄弟、零额外 API 调用。下次同类问题 FTS 直接命中该目录。

### 3. AI 读取回写

`gatherCloudContext` 中成功 `extractFileText/getTextContent` 的文件：`visit_count + 1`，`desc` 用"与问题的关联/内容主题"一句话回填（空缺才填）。

### 种子索引（seed.rs）

注册 drive 时入队（内存队列 + spawn），仅做一次 `GET /drives/{id}/root/children?$top=200`（分页读完根层，通常 1-2 页），写入顶层节点，`status: seeding→ready`。失败标记 failed，用户下次打开该 drive 的 tab 时重试。`catalog-event` 推进度。

### 手动深度索引（可选，不自动触发）

`catalog_reindex(account, drive_id, max_depth)`：BFS 遍历（复用 GraphClient 重试/限流，批量事务 upsert，可取消，进度事件）。供用户主动全量建图；默认不暴露在主流程，设置/调试入口提供。

## 查询（query.rs）

```text
query_catalog(keywords: Vec<String>, accounts: Option<Vec<(account_id, cloud_env)>>, limit)
  → Vec<CatalogHit { account, drive(含 site_name), path, item_id, name, kind, desc, visit_count }>
```

- 每个 ≥3 字符关键词构造 FTS5 `(name:*kw* OR path:*kw* OR desc:*kw*)`（trigram substring MATCH），多关键词 AND；全部 <3 字符回退 LIKE。
- 排序 `visit_count DESC, length(path) ASC`，LIMIT 默认 20。
- 账号过滤 `WHERE account_id IN (…)` 强制施加（隔离红线）。

## AI grounding v2（gatherCloudContext 改造）

```text
1. keywords = 问题切词（前端简单切分）
2. catalog_query(keywords, 选中账号) → hits
3. 候选 drive = hits 按 drive 聚合，visit 加权前 K=4
   ├─ 无 hits → 回退现状（各账号 OneDrive 全局搜索）
4. 每个候选 drive：对每个关键词各执行一次 searchFiles 合并去重（查询扩展）
5. 候选目录 → location_hints 注入系统提示词（"Likely locations: …"）
6. 完成后 catalog_record_hits(source='grounding') + AI 读取文件回写
```

`build_system_prompt(context_files, location_hints, memories)` 签名扩展（memories 段由 ai-memory spec 增补）。

## 命令清单

| 命令 | 说明 |
|---|---|
| `catalog_register_drive` | 注册（幂等）+ 入队根层种子 |
| `catalog_unregister_drive` | 注销 + 删除节点 |
| `catalog_status` | 注册表视图 |
| `catalog_record_browse` | 浏览回写（FileBrowser 调用） |
| `catalog_record_hits` | 搜索/grounding 命中回写 |
| `catalog_query` | FTS5 检索 |
| `catalog_reindex` / `catalog_reindex_all` | 手动深度索引（可选） |
| `catalog_cancel_index` | 取消手动索引 |

事件：`catalog-event`；Zustand `catalogStore` 订阅（状态/进度）。

## 正确性属性（测试锚点）

1. Property: 注册表以 (account, drive) 唯一 —— 重复注册只更新元数据。
2. Property: 节点 upsert 幂等且统计保留 —— 同路径重复写入不产生重复行，visit_count 跨写入累加。
3. Property: FTS 同步不变量 —— nodes 与 node_fts 行集一致（触发器覆盖 insert/update/delete）。
4. Property: 回写单调性 —— visit_count 只增；非空 desc 永不被覆盖。
5. Property: 查询隔离 —— 结果 account_id 恒属于请求集合（proptest 随机组合）。
6. Property: trigram 子串保证 —— name/path 中任意 ≥3 字符子串（含中文）必可检索。
7. Property: 祖先链完整 —— 命中回写后，该文件路径的每一级祖先都是可查询的 folder 节点。
8. Property: 种子幂等 —— 重复种子索引不改变节点集。

## 风险与开放问题

- **冷启动召回**：catalog 只有根层时，候选 drive 选择退化为"全部已注册 drive"（等价现状 + 文档库覆盖）；随使用快速改善。可接受。
- **浏览回写频率**：FileBrowser 每次进入目录都回写——单事务批量写，量级小（≤200 行/次）；仍做 300ms 去抖合并。
- **手动索引的深度上限**：默认 5、UI 上限 10，防误触超大库。
- **delta 可用性（spike）**：仅影响手动深度索引的增量优化，主路径（回写积累）不依赖。
