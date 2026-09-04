# 会议录像查看（meeting-recordings）任务

- [x] 1. models.rs 增加 `RecordingSource`、`MeetingRecording`（serde camelCase + 单测）
- [x] 2. graph/commands.rs 抽取 `discover_sites` 供两处共用（行为不变）
- [x] 3. graph/commands.rs 实现录像聚合辅助函数与 `get_meeting_recordings` 命令
  - [x] OneDrive `Recordings` 目录枚举（404 静默空）
  - [x] 站点驱动器 search('Recordings') → 容器 children 枚举（buffer_unordered 6，含限额常量）
  - [x] 去重 + createdDateTime 倒序排序
  - [x] 扩展名过滤与排序属性测试（proptest）
- [x] 4. lib.rs invoke_handler 注册
- [x] 5. types.ts 增加镜像类型；tauri.ts 增加 `getMeetingRecordings` 封装
- [x] 6. RecordingsPage 组件（列表/缩略图切换、过滤、刷新、播放与下载动作、空/载入/错误态）
- [x] 7. MainContent nav 步骤接线与双击→openPreviewTab 打通；DriveHubPage 第四入口卡片
- [x] 8. i18n en-US / zh-CN 增加 `recordings.*` 与 `driveHub.meetingRecordings`
- [x] 9. `cargo test` 与 `npm run build` 通过
- [ ] 10. 真机手工验收：global + 世纪互联账户、含频道会议录制站点（需要真实账号环境）

## 已知限制（后续迭代候选）

- **世纪互联暂不支持**（2026-08 决策）：入口对 china 账户置灰+提示，后端直接返回空集合。待官方 communications API 或等价通道在 21Vianet 可用时重估。
- 上限之外的更多站点/驱动器不参与聚合（100 sites / 5 drives / 10 containers per drive）。
- 组织为他人的"与我共享"型链接录像、聊天卡片自动发现未纳入（属参与者发现路线图第二期 `/shares` 解析 / Chat scope 方案）。
- 仅可在线观看（策略禁下载）场景依赖 preview 嵌入播放器可用性；条目悬停有「在浏览器打开」按钮直达 ODSP/Stream 播放页兜底。
- Graph communications API 备忘：`getAllRecordings` 委托权限 Not supported（仅 Application）；`callRecording` 内容委托仅组织者本人可用且为计费 API——已评估并排除出桌面端实现路线。
