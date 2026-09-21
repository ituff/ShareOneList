#!/usr/bin/env pwsh
<#
.SYNOPSIS
    ShareOneList 自动化发布脚本
.DESCRIPTION
    自动完成：更新版本号 → 构建发布 → 打包 zip → 发布到 GitHub Release
.PARAMETER Version
    版本号，格式 x.y.z，例如 1.8.0
.PARAMETER Notes
    Release 说明（可选），支持 Markdown。不提供则使用默认模板。
.PARAMETER Draft
    创建草稿 Release，不立即发布
.PARAMETER DryRun
    仅构建打包，不发布到 GitHub
.EXAMPLE
    .\publish.ps1 -Version 1.8.0
.EXAMPLE
    .\publish.ps1 -Version 1.8.0 -Notes "- 修复了某个 bug`n- 新增了某个功能"
.EXAMPLE
    .\publish.ps1 -Version 1.8.0 -DryRun
#>

param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$Version,

    [string]$Notes,

    [switch]$Draft,

    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

# ── 配置 ──────────────────────────────────────────────
$ProjectFile = "SimpleList/SimpleList.csproj"
$Runtime      = "win-x64"
$ProductName  = "ShareOneList"
$ZipName      = "$ProductName-v$Version-x64.zip"
$TagName      = "v$Version"
$RepoRoot     = $PSScriptRoot

# gh cli 路径
$GhExe = Get-Command gh -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
if (-not $GhExe) {
    $GhExe = "C:\Program Files\GitHub CLI\gh.exe"
}
if (-not (Test-Path $GhExe)) {
    Write-Error "未找到 GitHub CLI (gh)，请先安装: winget install GitHub.cli"
    exit 1
}

# ── 函数 ──────────────────────────────────────────────
function Write-Step($msg) {
    Write-Host "`n▸ $msg" -ForegroundColor Cyan
}

function Update-Version {
    Write-Step "更新版本号为 $Version"

    $csproj = Get-Content $ProjectFile -Raw

    # 更新 <Version>x.y.z</Version>
    $csproj = $csproj -replace '(<Version>)[^<]+(</Version>)', "`${1}$Version`${2}"
    # 更新 <AssemblyVersion>x.y.z</AssemblyVersion>
    $csproj = $csproj -replace '(<AssemblyVersion>)[^<]+(</AssemblyVersion>)', "`${1}$Version`${2}"

    Set-Content $ProjectFile -Value $csproj -NoNewline
    Write-Host "  Version=$Version, AssemblyVersion=$Version"
}

function Invoke-Publish {
    Write-Step "构建发布包 (Release, $Runtime, self-contained, single-file, trimmed)"

    dotnet publish $ProjectFile -c Release -r $Runtime --self-contained
    if ($LASTEXITCODE -ne 0) {
        Write-Error "dotnet publish 失败"
        exit 1
    }
}

function Copy-NativeDeps {
    Write-Step "复制原生依赖到 publish 目录"

    $buildDir   = "SimpleList/bin/x64/Release/net9.0-windows10.0.19041.0/$Runtime"
    $publishDir = "$buildDir/publish"

    # appsettings.json
    Copy-Item "SimpleList/appsettings.json" "$publishDir/appsettings.json" -Force

    # WinUI 3 / WinAppSDK 原生 DLL 匹配模式
    $nativePatterns = @(
        "Microsoft.ui.xaml*", "Microsoft.UI.*", "Microsoft.WindowsAppRuntime*",
        "Microsoft.Internal.*", "Microsoft.Web.WebView2.Core.dll", "MRM.dll",
        "dcompi.dll", "dwmcorei.dll", "DwmSceneI.dll", "DWriteCore.dll",
        "CoreMessagingXP.dll", "Microsoft.InputStateManager.dll", "Microsoft.DirectManipulation.dll",
        "wuceffectsi.dll", "WinUIEdit.dll", "marshal.dll", "Microsoft.Graphics.Display.dll",
        "PushNotificationsLongRunningTask*", "SessionHandle*",
        "WindowsAppRuntime*", "WindowsAppSdk*", "WebView2Loader.dll",
        "Microsoft.WinUI.dll", "RestartAgent.exe"
    )

    foreach ($pattern in $nativePatterns) {
        Get-ChildItem $buildDir -Filter $pattern -File -ErrorAction SilentlyContinue | ForEach-Object {
            if (-not (Test-Path (Join-Path $publishDir $_.Name))) {
                Copy-Item $_.FullName $publishDir
            }
        }
    }

    # .pri / .xbf / .winmd 资源文件
    foreach ($ext in @("*.pri", "*.xbf", "*.winmd")) {
        Get-ChildItem $buildDir -Filter $ext -File -ErrorAction SilentlyContinue | ForEach-Object {
            Copy-Item $_.FullName $publishDir -Force
        }
    }

    # Microsoft.UI.Xaml 资源目录
    if (Test-Path "$buildDir/Microsoft.UI.Xaml") {
        Copy-Item "$buildDir/Microsoft.UI.Xaml" "$publishDir/Microsoft.UI.Xaml" -Recurse -Force
    }

    # Assets 目录
    if (Test-Path "$buildDir/Assets") {
        Copy-Item "$buildDir/Assets" "$publishDir/Assets" -Recurse -Force
    }

    # 多语言资源目录
    foreach ($locale in @("en-us", "zh-CN")) {
        if (Test-Path "$buildDir/$locale") {
            Copy-Item "$buildDir/$locale" "$publishDir/$locale" -Recurse -Force
        }
    }

    $fileCount = (Get-ChildItem $publishDir -Recurse -File | Where-Object { $_.Extension -ne ".pdb" }).Count
    Write-Host "  publish 目录共 $fileCount 个文件（不含 pdb）"
}

function New-ZipPackage {
    Write-Step "打包 $ZipName"

    $publishDir = "SimpleList/bin/x64/Release/net9.0-windows10.0.19041.0/$Runtime/publish"
    $zipPath = Join-Path $RepoRoot $ZipName

    if (Test-Path $zipPath) { Remove-Item $zipPath }

    $items = Get-ChildItem $publishDir | Where-Object { $_.Extension -ne ".pdb" }
    Compress-Archive -Path $items.FullName -DestinationPath $zipPath -CompressionLevel Optimal

    $sizeMB = [math]::Round((Get-Item $zipPath).Length / 1MB, 1)
    Write-Host "  $ZipName ($sizeMB MB)"
}

function Publish-GitHubRelease {
    if ($DryRun) {
        Write-Step "DryRun 模式，跳过 GitHub 发布"
        Write-Host "  构建产物: $ZipName"
        return
    }

    Write-Step "发布到 GitHub Release ($TagName)"

    # 检查 tag 是否已存在
    $existing = & $GhExe release view $TagName 2>&1
    if ($LASTEXITCODE -eq 0) {
        Write-Warning "Release $TagName 已存在，跳过发布。如需覆盖请先手动删除。"
        return
    }

    # 默认 release notes
    if (-not $Notes) {
        $Notes = "## $ProductName $TagName`n`n发布日期: $(Get-Date -Format 'yyyy-MM-dd')"
    }

    $ghArgs = @("release", "create", $TagName, $ZipName, "--title", "$ProductName $TagName", "--notes", $Notes)
    if ($Draft) {
        $ghArgs += "--draft"
    }

    & $GhExe @ghArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Error "GitHub Release 创建失败"
        exit 1
    }

    Write-Host "`n✅ 发布完成!" -ForegroundColor Green
}

# ── 主流程 ────────────────────────────────────────────
Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor DarkGray
Write-Host "  $ProductName 发布脚本  v$Version" -ForegroundColor White
Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor DarkGray

Update-Version
Invoke-Publish
Copy-NativeDeps
New-ZipPackage
Publish-GitHubRelease

Write-Host ""
