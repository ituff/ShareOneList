# Drive Catalog 检索层 — 需求文档

## 概述

为 AI 助手与全局搜索提供跨站点、跨文档库的云端文件定位能力：以 **drive**（OneDrive 或 SharePoint 文档库）为作用域单元维护本地目录地图（SQLite + FTS5）。地图**不做预先深度爬取**，而是跟随用户真实行为渐进积累——浏览过、检索命中过的目录逐步入库——最终收敛到用户实际工作集的"完整站点目录"。来源于对福斯 TECH AI 站点地图 skill 的产品化改造。

明确不做：本地 RAG / 向量索引 / 文件内容全文索引（DEV_PLAN 既有决策）；**默认的全量后台爬取**（v2 讨论决策：五层 BFS 不作为默认行为，仅保留手动深度索引）。地图只存**元数据**（路径、名称、描述、访问计数），不存文件内容。

## 背景与动机

当前 AI grounding 直接对每个账号的主 OneDrive 调 `search(q=问题原文)`：

- Graph 搜索对中文长句命中率低（skill 实测结论），召回差；
- 组织版用户可达的 SharePoint 站点文档库完全不在检索范围内；
- 大库（数百 GB、数万节点）预先爬取既慢又大部分无用——用户 99% 的工作集中在一小撮目录。

因此采用**冷启动浅种子 + 使用驱动积累**：注册时只索引根层（1 次 API 调用，拿到顶层分区），此后目录知识完全来自用户行为。

## 需求

### 需求 1：Drive 注册（按需）

**验收标准**

1. THE 系统 SHALL 为每个已登录账号自动注册其 OneDrive 主 drive。
2. WHEN 用户在文件页打开一个尚未注册的 SharePoint 文档库时，THE 系统 SHALL 自动注册该 drive。
3. THE 系统 SHALL 维护 drive 注册表：账号、云环境、drive id、类型（onedrive/documentLibrary）、所属站点名、状态、节点数、最近使用时间。
4. THE 系统 SHALL 支持手动注销一个 drive（连同其节点数据）。

### 需求 2：渐进积累（不预爬，跟随使用）

**验收标准**

1. WHEN 一个 drive 注册时，THE 系统 SHALL 立即执行**根层种子索引**（仅 1 次根目录 children 调用），使 catalog 从一开始就知道顶层分区（如 R&D / AE&TS / Lab / PM…）。
2. WHEN 用户在文件页浏览某文件夹（list_files 成功返回）时，THE 系统 SHALL 把该文件夹的子项写入 catalog（路径、item id、名称、类型），并对该文件夹执行一次访问回写（visit_count +1）。
3. WHEN 全局搜索或 AI grounding 命中文件时，THE 系统 SHALL 把命中文件与其**祖先目录链**写入 catalog（祖先仅路径节点，不枚举兄弟、不发额外 API 调用）。
4. 系统 SHALL NOT 默认执行超过根层的后台遍历；根层种子 SHALL 在注册后台任务中完成并推送 `catalog-event` 进度。
5. THE 系统 SHALL 提供可选的手动深度索引（单 drive，用户指定深度），供用户主动全量建图；此为显式操作，不自动触发。

### 需求 3：节点存储与全文检索（FTS5）

**验收标准**

1. THE 系统 SHALL 存储 drive 内的文件夹与文件节点：路径（相对 drive 根）、item id、名称、类型、描述、最近访问时间、访问计数。
2. THE 系统 SHALL 对名称/路径/描述建立 FTS5 全文索引（trigram 分词），任意 ≥3 字符的子串必须可命中（含中文）。
3. WHEN 查询词短于 3 字符时，THE 系统 SHALL 回退到 LIKE 匹配。
4. 查询结果 SHALL 按 访问计数 + 路径长度 排序，并可按账号集合过滤；绝不允许跨出请求的账号范围（双云隔离红线延伸到本地缓存）。
5. 重复写入同一路径 SHALL 幂等：以 (账号, drive, 路径) 为主键 upsert，访问统计跨 upsert 保留。

### 需求 4：访问回写（自学习核心）

**验收标准**

1. 以下三类行为 SHALL 回写 catalog：浏览（该文件夹 visit_count +1）、搜索/grounding 命中（文件与其祖先链写入并计数）、AI 读取文件内容（该文件 visit_count +1，desc 用内容主题回填）。
2. `visit_count` SHALL 只增不减；空缺 `desc` SHALL 被回填，非空描述不被覆盖。
3. THE 系统 SHALL 在 usage 台账记录每次回写的来源（browse/search/grounding）与明细，保证"答案来自哪里"可审计。
4. 回写 SHALL 为尽力而为：失败不影响浏览/聊天/搜索主流程。

### 需求 5：AI grounding 集成

**验收标准**

1. WHEN 用户在问 AI 提问时，THE 系统 SHALL 先用问题关键词查询 catalog，得到候选 drive 与候选目录。
2. THE 系统 SHALL 对候选 drive（按 visit 加权取前 K 个，默认 4）执行 Graph `search(q=)`（多组同义词扩展）；catalog 无命中时行为回退到现状（仅各账号 OneDrive）。
3. 候选目录路径 SHALL 注入系统提示词作为"可能位置"提示。
4. grounding 完成后 SHALL 触发需求 4 的命中回写。

### 需求 6：目录新鲜度

**验收标准**

1. WHEN 用户浏览某个目录时，THE 系统 SHALL 以浏览到的实时内容刷新 catalog 中该目录的子项（重命名/删除/新增自然同步——浏览即刷新）。
2. THE 系统 SHALL 提供手动深度索引与删除重建命令（单 drive / 全部）。
3. delta 增量刷新（`/drives/{id}/root/delta`）作为验证项：双云可用则用于手动深度索引的增量优化，不可用则全量（见任务 spike）。

### 需求 7：数据安全与隔离

**验收标准**

1. catalog 数据库为派生数据：损坏时允许整体删除重建（随后由使用行为重新积累），不影响配置与聊天历史。
2. 仅记录当前会话 token 可读的 drive（注册来源即用户可见可浏览的库），不引入额外提权。
3. 所有用户可见文案（状态、进度、设置）走 i18n（en-US / zh-CN）。

## 范围外

- 文件内容索引 / RAG / embedding（既有决策）
- 默认后台深度爬取（v2 讨论决策：仅手动触发）
- 文件夹比较 + 同步（DEV_PLAN 独立特性，未来复用 catalog）
- 站点自动发现建图（保持按需注册；全量枚举在 21V 不可行）
