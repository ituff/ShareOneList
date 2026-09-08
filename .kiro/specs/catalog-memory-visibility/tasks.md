# 可视化 — 任务清单

## 阶段一：设置页子 tab（本迭代完成）

- [x] 1. SettingsPage 拆四个 tab（外观与语言 / 下载 / AI 助手 / 关于），tab 状态提升到 settingsStore
- [x] 2. i18n：tab 标签（en-US / zh-CN）
- [x] 3. npm run build 验证

## 阶段二：记忆披露

- [x] 4. 后端：`LlmChatStart { request_id, used_memories }` 返回类型 + llm_chat 组装
- [x] 5. 前端：llmChat 返回类型、ChatEntry.usedMemories、回答下方「已结合 N 条记忆」展开条 + 去编辑跳转（settingsStore.settingsTab = "ai"）
- [x] 6. i18n + 构建验证

## 阶段三：站点地图页

- [x] 7. 后端：`catalog_tree`（按 parent_path 查子节点）+ `catalog_usage_recent`（台账摘要）+ 单测
- [x] 8. 前端：navigationStore + Sidebar「站点地图」入口；SitemapPage（drive 列表 + 状态徽章 + 积累摘要 + 操作按钮）
- [x] 9. CatalogTree 懒加载树（热度底色 + 🔥×N）+ 检索模式
- [x] 10. catalogStore 订阅 catalog-event（深度索引进度条 + 取消）
- [x] 11. i18n + 构建验证

## 验收清单（人工）

- [x] 设置页四个 tab 切换正常，无长滚动
- [x] 聊天回答下方出现「已结合 N 条记忆」，展开可见原文并可跳转编辑
- [x] 站点地图页列出已注册 drive，树视图随浏览积累增长，热度可见
- [x] 检索模式命中与 catalog_query 一致
- [x] 手动深度索引进度条实时、可取消
