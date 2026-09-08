# 可视化 — 设计文档

## 1. 站点地图页

```text
前端：
  navigationStore: + "sitemap" section
  Sidebar: Search icon → Map icon「站点地图」
  components/sitemap/SitemapPage.tsx    # 页面骨架 + drive 列表 + 模式切换
  components/sitemap/CatalogTree.tsx    # 懒加载目录树
  stores/catalogStore.ts                # 驱动列表状态 + catalog-event 订阅（状态/进度）
```

### 后端新命令

| 命令 | 说明 |
|---|---|
| `catalog_tree` | 输入 (account_id, drive_id, parent_path)，返回直接子节点（nodes 表按 parent_path 查询，idx_nodes_parent 已有索引）；根用 parent_path = '' |
| `catalog_usage_recent` | 返回 usage 表最近 N 条聚合摘要（按 source 分组计数 + 最近问题列表） |

llm_chat 之外的现有命令（catalog_status / catalog_query / catalog_reindex / catalog_cancel_index / catalog_unregister_drive）直接复用。

### 交互细节

- Drive 列表选中 → 主区加载该 drive 的树（根 = parent_path ''）。
- 树节点：文件夹行显示名称 + 🔥×visit_count（>0 时）+ 子节点计数徽章；点击展开时调 `catalog_tree` 懒加载。叶文件节点显示名称 + 计数。
- 热度信号：visit_count ≥5 深底色，1–4 浅底色，0 无色（默认展开到深度 2，其余点击加载）。
- 检索模式：输入 ≥1 关键词 → catalog_query（限当前账号 scope）→ 结果列表（路径 + site + 🔥×count），点击路径复制（第一版不做跳转预览，命中 item_id 可能为空的祖先节点）。
- 深度索引：进度条数据来自 `catalog-event`（visited_nodes / current_path / status），catalogStore 订阅；完成/取消/失败更新 drive 状态。

## 2. 记忆披露

- `llm_chat` 返回类型从 `String`（request_id）改为结构体：

```rust
#[serde(rename_all = "camelCase")]
pub struct LlmChatStart {
    pub request_id: String,
    /// 实际注入本轮提示词的记忆内容（空 = 未使用）。
    pub used_memories: Vec<String>,
}
```

- 前端 `llmChat()` 返回类型同步；ChatEntry 增加 `usedMemories?: string[]`（挂在 assistant 消息上）。
- 渲染：回答气泡下方（引用卡片之上）淡色一行「已结合 N 条记忆 ⌄」，点击展开列出原文；每条附「去编辑」链接 → `setActiveSection("settings")`（设置页默认落在 AI 助手 tab，见下）。
- memories 注入逻辑不变，仅把已选列表带出。

## 3. 设置页子 tab

- `SettingsPage` 内部 `useState<"appearance" | "downloads" | "ai" | "about">`，顶部 tab 栏（按钮组样式，选中态 bg-primary）：
  - **外观与语言**：主题、语言
  - **下载**：分段并发
  - **AI 助手**：LlmSettings + MemorySettings
  - **关于**：版本 / GitHub / 公众号 / UpdateChecker
- i18n：`settings.tabAppearance / tabDownloads / tabAI / tabAbout`。
- 记忆披露的「去编辑」跳设置后需落在 AI tab：settings tab 状态提升到 settingsStore（`settingsTab` 字段），MemorySettings 跳转时设置它。

## 测试锚点

- `catalog_tree`：父子关系正确、根查询、隔离（沿用 store 测试基建）。
- `catalog_usage_recent`：分组计数正确。
- `LlmChatStart` 序列化 camelCase；未启用记忆时 used_memories 为空。
- 设置页：四 tab 均可切换渲染（手工验收）。
