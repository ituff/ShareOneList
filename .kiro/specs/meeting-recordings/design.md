# 会议录像查看（meeting-recordings）设计

## 架构总览

遵循「Rust 拥有所有 Graph 访问，前端仅表现层」原则：

```
Files → 账户 → DriveHubPage(第4卡片 会议录像)
  └─ RecordingsPage
       ├─ getMeetingRecordings(cloudEnv, homeAccountId)   ← 新增命令，一次 IPC 聚合
       │     Rust: discover_sites() 复用
       │          + /me/drive/root:/Recordings/children
       │          + /sites/{id}/drives × /drives/{did}/root/search(q='Recordings')
       │               × /drives/{did}/items/{cid}/children
       ├─ 列表 / 缩略图 视图（本地过滤）
       └─ 双击 → openPreviewTab(rec.item, rec.driveId, ...) → PreviewPage 既有播放链路
            preview API 嵌入 → downloadUrl <video> 兜底（已在 PreviewPage 实现）
```

## 数据模型

Rust `models.rs` 新增（与 `src/lib/types.ts` 同步）：

```rust
pub enum RecordingSource { OneDrive, SharePoint } // serde lowercase

pub struct MeetingRecording {
    pub drive_id: String,
    pub item: DriveItem,
    pub source_type: RecordingSource,
    pub source_name: String, // OneDrive 为空串，SharePoint 为站点 displayName
}
```

前端仅依赖 `rec.driveId + rec.item` 即可接入既有缩略图/下载/预览（均按 driveId+itemId 寻址）。

## 后端

新命令 `graph::commands::get_meeting_recordings(cloud_env, home_account_id)`：

1. `get_token_for_account` 取令牌（失败向上传播 → 认证错误路径）。
2. OneDrive 来源：`GET {base}/me/drive/root:/Recordings/children?$top=200&$select={DRIVE_ITEM_SELECT}` 分页；非文件夹且扩展名属于视频集合者入选。
3. 站点来源：
   - 抽取 `discover_sites(client, env, token) -> Vec<Site>`（原 `get_sharepoint_sites` 主体：followedSites+search+China memberOf groups+root 兜底，去重、吞错），原命令与新命令共用。
   - 并发处理站点（`futures::stream::iter(...).buffer_unordered(6)`），单站点内顺序执行：`GET /sites/{id}/drives` → 对每个驱动器 `GET /drives/{did}/root/search(q='Recordings')` → 过滤 folder facet 且名称等于 "Recordings"（大小写不敏感）的容器 → 枚举其 children 收集视频。
4. 聚合后去重（`(drive_id,item.id)`）、按 `createdDateTime` 倒序（chrono RFC3339 解析，失败回退 `lastModified` 再回退 0，同键名升序）。

### 错误与限额语义

- **令牌前**错误传播；**聚合中**任何站点/驱动器/目录错误一律跳过（返回部分结果）——受限文件、无权限站点不应造成整页失败。OneDrive 目录 404（无录像文件夹）同样静默为空。
- 常量上限：`MAX_RECORDING_SITES=100`、`MAX_DRIVES_PER_SITE=5`、`RECORDING_CONTAINERS_PER_DRIVE=10`、`SITE_CONCURRENCY=6`。目的：防止 memberOf 大租户下请求爆炸；超过上限内容本期不展示（在任务清单中记录为已知限制）。

### 正确性测试（commands.rs 内参数化单测，仓库未引入 proptest 依赖）

- 视频扩展名过滤器：集合内扩展名（`.mp4/.m4v/.mov/.mkv/.webm/.avi/.wmv`）大小写不敏感通过；`.vtt/.txt`、无扩展名、点在名称中部的一律拒绝。
- 排序函数：任意输入下 createdDateTime 新者在前，解析失败者垫底，同键名称字典序（大小写不敏感）。

## 前端

### 导航

- `MainContent.tsx` 的 `FilesNavState` 增加 `{ step: "recordings"; account: AccountEntry }`；`navAccountHomeId` 条件同步覆盖该步。
- `DriveHubPage` 第四卡片 `Video` 图标 + `t("driveHub.meetingRecordings")`，回调经 FilesPage 进入 recordings 步骤。

### RecordingsPage（`src/components/files/RecordingsPage.tsx`）

- props：`account`、`onOpenRecording(rec)`、`onBack()`；挂载时调用 `getMeetingRecordings`。
- 工具栏：返回、刷新、过滤输入框、视图切换（list | thumbnails）。视图状态本地 `useState`（页面无常驻 tab，不入 settingsStore）。
- 列表视图列：名称 / 来源 / 大小 / 创建时间；行尾悬停操作按钮：播放（双击等价）、下载。
- 缩略图视图：180px 卡片，28 高度首帧区（`getThumbnailUrl` 失败回退 `MonitorPlay` 图标），下方名称 + 来源小字。
- 缩略图缓存沿用 FileItem 的模块级 Map 思路，key `${cloudEnv}:${driveId}:${itemId}`。
- 本地过滤：name/sourceName 小写包含。
- 下载复用单文件流：`save()` 对话框 → `downloadFile(...)` → `taskStore.registerTask`。

### 播放 Tab

- 双击 → `openPreviewTab(rec.item, rec.driveId, cloudEnv, homeAccountId)`，零改动复用既有预览 Tab 与 `PreviewPage` 三级降级（preview 嵌入 → downloadUrl 直链 → webUrl）。
- 已知边界：preview 嵌入播放器可用性随租户策略浮动；全部降级失败显示错误态并重试。真正的截流合成方案不在本 spec 范围（见 requirements 背景）。

## 测试策略

- Rust 单元测试（commands.rs）：视频扩展过滤、聚合排序、MeetingRecording camelCase 序列化；两个 proptest 属性。
- TypeScript：`npm run build` 类型检查兜底（strict）；组件级逻辑（过滤/格式化）保持简单不引入测试框架变更。
- 手工验收矩阵：global/china × 有无 OneDrive 录像 × 频道会议站点。
