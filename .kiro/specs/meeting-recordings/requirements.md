# 会议录像查看（meeting-recordings）需求

## 背景

Teams 会议录像在 Stream 经典版退役后以普通文件形式存储：非频道会议录制保存在组织者 OneDrive 的 `Recordings` 文件夹；频道会议录制保存在团队 SharePoint 站点文档库的 `Recordings` 文件夹内。用户（尤其是作为参与者）需要一个统一页面查看自己有权限访问的全部录像。

**范围决定（2026-08-27）**
- 本功能**暂不支持世纪互联账户**：入口禁用、后端短路。Graph communications 系 API 在 21Vianet 不可用。
- 国际版不采用 Graph communications 枚举（`getAllRecordings` 的委托权限为 Not supported，仅 Application 权限；`callRecording` 内容委托下仅组织者本人可取且为计费 API——纯桌面客户端不可依赖）。以 Drive 聚合为主干；每条录像提供直达 ODSP/Stream 播放页的浏览器链接。
- 「列出参与者可访问的全部会议」的完整方案待定：候选为聊天卡片发现（Chat scope）与共享链接 `/shares` 解析。

本任务只做「查看与在线播放」。下载能力天然由现有下载引擎继承（页内提供下载入口）；对租户策略禁用下载的受限场景，本期通过 Graph preview 嵌入播放器降级支持在线观看，不做流媒体截流合成（见 design 的范围外事项）。

## 用户故事与验收标准

### US1：入口

**WHEN** 用户进入「文件」并双击某个账户 **THEN** 系统展示的服务选择页包含与 OneDrive、SharePoint、与我共享并列的第四张卡片「会议录像」。
**WHEN** 该账户没有 OneDrive 或没有可发现的站点 **AND** 用户点击「会议录像」 **THEN** 页面仍正常打开并展示空状态，不报错弹窗。

### US2：聚合列出

**WHEN** 用户打开「会议录像」页面 **THEN** 系统在一次请求内聚合返回：
- 该账户 OneDrive `Recordings` 文件夹下的视频文件；
- 用户可发现的所有 SharePoint 站点中名为 `Recordings` 的文件夹内的视频文件（频道会议录制）。

**约束**
- 仅列出文件 facet 且扩展名属于视频集合的条目；`.vtt` 字幕等其它文件不显示。
- 每条记录标注来源（OneDrive 或站点名称）。
- 列表按创建时间倒序，同时间按名称排序。
- 单个站点或驱动器查询失败不导致整页失败（静默跳过该来源）。
- 两个云环境（global / china）行为一致；世纪互联下沿用 M365 Groups 站点发现流程。

### US3：视图切换

**WHEN** 用户点击工具栏视图切换 **THEN** 可在列表视图（名称/来源/大小/创建时间列）与缩略图视图（视频首帧大图卡片）之间切换；默认列表。缩略图加载失败回退到类型图标。

### US4：筛选

**WHEN** 用户在过滤框输入文本 **THEN** 前端本地按名称与来源即时过滤当前聚合结果。

### US5：双击在线播放

**WHEN** 用户双击任一录像条目 **THEN** 系统新开一个预览 Tab 在线播放该录像：
- 优先使用 Graph preview API 返回的嵌入播放器（适用于多数仅可在线观看的受限文件）；
- 失败时回退 `@microsoft.graph.downloadUrl` 直连流式播放；
- 再失败则展示错误态并提供重试。

### US6：下载入口（继承能力）

**WHEN** 用户点击某录像的下载动作 **THEN** 走既有单文件下载流程（保存对话框 + 批量任务事件），失败时 toast 提示（例如策略禁下载返回 403）。

## 错误处理

- 初始令牌获取失败按现有认证错误路径处理（触发重新登录模态）。
- 聚合阶段任意来源失败不产生错误 UI，仅可能减少结果数量。
- 播放/下载错误按现有 preview/download 错误模式呈现。

## 性能与限额（见 design）

- 参与聚合并发 ≤6，每站点最多枚举 5 个驱动器、每个驱动器最多取 10 个 `Recordings` 容器，站点总数上限 100。

## 2026-09-04 范围调整

- 功能更名「Teams 录像」（"Teams Recordings"），入口卡片、页面标题、标签页同步改名。
- 列表只保留两个来源：
  1. **本人**：用户 OneDrive 中 `Recordings` 文件夹下的 `.mp4` 文件。文件夹名随用户 UI 语言本地化（Recordings / 会议录制 / 录制 / Grabaciones / Enregistrements / Aufzeichnungen / Registrazioni / Gravações / 録画 / 녹화），逐一探测。
  2. **分享**：Microsoft Search（`filetype:mp4`）列出分享给用户的 `.mp4` 文件，排除用户自身 drive 的命中。
- SharePoint 站点频道录制枚举（discover_sites + 站点 drives 遍历）移除。
- 扩展名过滤收紧为仅 `.mp4`。
- 列表按 **修改时间** 倒序（原为创建时间）。
- 来源列显示「本人 / 分享」（`RecordingSource::Own | Shared`）。
- 列表不再提供下载按钮（在线预览与「在浏览器打开」保留）。
