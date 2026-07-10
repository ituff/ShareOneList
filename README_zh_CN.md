# ShareOneList

[English](./README.md) | 简体中文

> OneDrive 与 SharePoint，国际版与世纪互联——一个应用，连接所有云。

ShareOneList 是一个基于 WinUI 3 的 Microsoft 365 文件管理客户端，统一访问 OneDrive 和 SharePoint 文档库，同时支持国际版和世纪互联版。

## 特点

- **双云支持** — 在同一个应用中管理国际版和世纪互联版 Microsoft 365 的文件
- **OneDrive + SharePoint** — 个人 OneDrive、SharePoint 站点文档库、共享文档库一站式浏览
- **多账户** — 支持添加多个不同云环境的账户
- **批量下载** — 通过复选框勾选文件和文件夹，一键批量下载
- **任务管理** — 实时查看下载和上传进度
- **文件预览** — 在应用内预览图片、Markdown 和媒体文件
- **文件操作** — 上传、下载、重命名、删除、分享、格式转换
- **深色模式** — 跟随系统主题，支持 Mica / Acrylic 材质
- **多语言** — 支持英文和简体中文

## 使用方法

1. 从 [Releases](https://github.com/ituff/SimpleList21V/releases) 下载最新版本
2. 解压后运行 `ShareOneList.exe`
3. 点击左侧菜单栏的 **文件**，然后点击 **添加网盘** 登录你的 Microsoft 账户
4. 双击网盘进入文件浏览

## 配置

编辑 `appsettings.json` 设置你自己的 Azure AD 客户端 ID：

```json
{
  "AzureAD": {
    "Global": {
      "ClientId": "你的国际版客户端ID"
    },
    "China": {
      "ClientId": "你的世纪互联版客户端ID"
    }
  }
}
```

> **注意：** 国际版和世纪互联版的 Azure AD 是完全独立的体系，你需要分别在 [portal.azure.com](https://portal.azure.com)（国际版）和 [portal.azure.cn](https://portal.azure.cn)（世纪互联版）注册应用。

## 功能列表

- [x] OneDrive 文件浏览
- [x] SharePoint 站点与文档库浏览
- [x] 国际版与世纪互联版支持
- [x] 多账户管理
- [x] 复选框批量下载
- [x] 下载 / 上传进度跟踪
- [x] 文件分享与链接生成
- [x] 文件预览（图片、Markdown、媒体）
- [x] 重命名 / 删除 / 属性查看
- [x] 转换为 PDF
- [x] 存储容量显示
- [x] 详细 / 缩略图 / 看图布局模式
- [x] 拖拽上传
- [x] 深色模式与主题自定义
- [x] 英文 / 简体中文
- [ ] 自动同步

## 截图

> 可能不是最新版本。

![HomePage](./ScreenShots/HomePage.png)
![CloudPage](./ScreenShots/CloudPage.png)
![DrivePage](./ScreenShots/DrivePage.png)
![CreateFolder](./ScreenShots/CreateFolder.png)
![GridLayout](./ScreenShots/GridLayout.png)
![Download](./ScreenShots/Download.png)
![Share](./ScreenShots/Share.png)
![ImageViewing](./ScreenShots/ImageViewing.png)
![ToolsPage](./ScreenShots/ToolsPage.png)
![DarkMode](./ScreenShots/DarkMode.png)

## 致谢

ShareOneList 基于 [aiguoli/SimpleList](https://github.com/aiguoli/SimpleList) 开发。感谢原作者的开源贡献。
