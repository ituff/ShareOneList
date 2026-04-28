---
inclusion: always
---

# 项目概述

SimpleList 是一个基于 WinUI 3 的 OneDrive / SharePoint 文件管理客户端，fork 自 [aiguoli/SimpleList](https://github.com/aiguoli/SimpleList)，在原项目基础上增加了世纪互联（21Vianet）版 Microsoft 365 的支持。

- **仓库地址：** https://github.com/ituff/SimpleList21V
- **框架：** WinUI 3 (Windows App SDK)，.NET 9，C#
- **MVVM 框架：** CommunityToolkit.Mvvm（使用 `[ObservableProperty]`、`[RelayCommand]` 等源生成器）
- **Graph SDK：** Microsoft.Graph v5（Kiota 风格 API）
- **认证：** MSAL（Microsoft.Identity.Client），公共客户端应用

# 架构

```
SimpleList/
├── Models/          # 数据模型（CloudType、DTO、FileType 等）
├── Services/        # OneDrive/Graph API 服务层
│   ├── OneDrive.cs          # Graph API 调用（文件操作、SharePoint、认证）
│   └── OneDriveServiceBase.cs  # 基类（错误处理、参数校验）
├── ViewModels/      # MVVM ViewModel 层
├── Views/           # ContentDialog 和子视图
├── Pages/           # 页面（导航目标）
│   ├── CloudPage        # 账户列表页
│   ├── DriveHubPage     # 服务选择页（OneDrive / SharePoint / 共享）
│   ├── DrivePage        # 文件浏览页
│   └── ...
├── Helpers/         # 工具类（配置、资源、主题）
├── Converters/      # XAML 值转换器
├── Controls/        # 自定义控件
├── Strings/         # 多语言资源（en-US、zh-CN）
└── Assets/          # 图标和图片
```

# 双云架构

项目同时支持国际版和世纪互联版，核心差异集中在 `Models/CloudType.cs`：

| 配置项 | 国际版 (Global) | 世纪互联版 (China) |
|--------|----------------|-------------------|
| Authority | login.microsoftonline.com/common | login.partner.microsoftonline.cn/organizations |
| Graph API | graph.microsoft.com/v1.0 | microsoftgraph.chinacloudapi.cn/v1.0 |
| SharePoint 域名 | sharepoint.com | sharepoint.cn |
| Scopes | 简写形式 (User.Read) | 完整 URI 前缀 |
| ClientId | appsettings.json → AzureAD.Global.ClientId | appsettings.json → AzureAD.China.ClientId |

两个版本的 Azure AD 完全独立，需要分别在 portal.azure.com 和 portal.azure.cn 注册应用。

# 世纪互联 API 限制

以下 Graph API 在世纪互联环境下不可用或行为不同：
- `/sites?search=*` — 不支持通配符搜索，报语法错误
- `/me/followedSites` — 依赖 OneDrive 许可证（MySite），无许可证报 "User's mysite not found"
- `/me/drive/sharedWithMe` — 同样依赖 OneDrive 许可证

当前 SharePoint 站点发现使用 M365 Groups 方式（`/me/memberOf` → 筛选 Unified Groups → `/groups/{id}/sites/root`）。

# 导航流程

```
CloudPage（账户列表）
  └─ 双击账户 → DriveHubPage（服务选择）
       ├─ OneDrive → DrivePage（文件浏览）
       ├─ SharePoint → 站点列表 → 文档库列表 → DrivePage
       └─ 与我共享 → 文档库列表 → DrivePage
```

# 构建与运行

```bash
# 构建
dotnet build SimpleList/SimpleList.csproj

# 运行
dotnet run --project SimpleList/SimpleList.csproj
# 或直接运行 exe
SimpleList\bin\x64\Debug\net9.0-windows10.0.19041.0\win-x64\SimpleList.exe
```

# 多语言

所有用户可见的文本必须使用 `x:Uid` 绑定到 `Strings/{locale}/Resources.resw`，不允许在 XAML 或代码中硬编码用户可见文本。当前支持 en-US 和 zh-CN。

# 编码规范

- 遵循项目现有的代码风格和命名约定
- ViewModel 使用 CommunityToolkit.Mvvm 源生成器
- Graph API 调用统一通过 `OneDrive` 服务类，使用 `ExecuteAsync` 包装以获得统一的错误处理
- 新增功能需要同时更新中英文资源文件
