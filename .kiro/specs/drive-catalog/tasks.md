# Drive Catalog 检索层 — 任务清单

## 阶段一：存储与回写（可独立验证）

- [x] 1. Rust `catalog/store.rs`：schema v1（drives / nodes / node_fts(FTS5 trigram + 触发器) / usage）、迁移、注册表 CRUD
  - [x] 1.1 建库与 user_version 迁移（幂等）
  - [x] 1.2 register / unregister / status（含节点级联清理）
  - [x] 1.3 节点批量 upsert（事务批处理，统计保留）
  - [x] 1.4 Property 1（注册唯一）、Property 2（upsert 幂等+统计累加）、Property 3（FTS 同步）测试
- [x] 2. Rust `catalog/writeback.rs`：三类回写
  - [x] 2.1 record_browse（浏览回写：文件夹计数 + 子项 upsert 同步）
  - [x] 2.2 record_hits（命中回写：文件 + 祖先链补全）—— Property 7 测试
  - [x] 2.3 AI 读取回写（desc 回填）—— Property 4（单调性）测试
  - [x] 2.4 usage 台账（source 分类）
- [x] 3. Rust `catalog/query.rs`：FTS5 trigram 查询 + LIKE 回退 + 排序 + 账号过滤
  - [x] 3.1 关键词切分与 MATCH 构造、排序与 LIMIT
  - [x] 3.2 Property 5（账号隔离）、Property 6（trigram 子串保证，含中文）测试
- [x] 4. Rust `catalog/seed.rs`：根层种子（1 次 children 分页读取）+ 手动深度索引（BFS、可取消、进度事件）
  - [x] 4.1 种子入队与状态机（queued→seeding→ready/failed）
  - [x] 4.2 Property 8（种子幂等，经 upsert 幂等覆盖）测试；Graph 解析（路径前缀剥离、folder/file 识别）测试
- [x] 5. `catalog/commands.rs` + lib.rs 注册（Arc<CatalogStore> + CatalogCancels）

## 阶段二：接入

- [x] 6. 注册触发：登录/加载账号后注册 OneDrive（authStore → catalogTriggers）；DriveList 列出站点文档库时注册
- [x] 7. 浏览回写接入：FileBrowser list_files 成功后 fire-and-forget catalog_record_browse（300ms 去抖）
- [x] 8. AI grounding v2：catalog_query 选 drive（前 K=4）+ 多关键词查询扩展 + location_hints 注入提示词 + 命中/读取回写
  - [x] 8.1 `build_system_prompt` 增加 location_hints 段（含测试更新）
  - [x] 8.2 `llm_chat` 参数扩展 + types.ts / tauri.ts
  - [x] 8.3 gatherCloudContext 改造 + 无 catalog 回退路径
- [ ] 9.（可选）搜索页接入：搜索前 catalog_query 提供目录提示
- [ ] 10.（spike）双云 `/drives/{id}/root/delta` 实测：仅影响手动深度索引优化；不可用则记录结论
- [x] 11. cargo test + npm run build 验证（spec 设置 UI 未做，无新增文案）

## 验收清单（人工）

- [ ] 登录账号 → OneDrive 注册并完成根层种子（秒级，无长时间后台任务）
- [ ] 浏览若干目录后 → catalog_query 能命中这些目录；未浏览过的目录不出现
- [ ] 问 AI 一个仅在 SharePoint 库中命中的关键词 → 回答引用该库文件（现状搜不到）
- [ ] 同一问题第二次提问 → 命中目录 visit_count 提升、排序前移
- [ ] 删除 catalog.db 后重启 → 根层种子自动恢复，随浏览重新积累；聊天历史不受影响
- [ ] 手动深度索引可选执行且可取消
