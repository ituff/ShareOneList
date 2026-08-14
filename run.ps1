#!/usr/bin/env pwsh
<#
.SYNOPSIS
    编译并运行 ShareOneList
.PARAMETER Configuration
    编译配置，Debug 或 Release，默认 Debug
.PARAMETER NoBuild
    跳过编译，直接运行上次的构建产物
.EXAMPLE
    .\run.ps1
.EXAMPLE
    .\run.ps1 -Configuration Release
.EXAMPLE
    .\run.ps1 -NoBuild
#>

param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Debug",

    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"
$ProjectFile = "SimpleList/SimpleList.csproj"
$ExePath = "SimpleList/bin/x64/$Configuration/net9.0-windows10.0.19041.0/win-x64/ShareOneList.exe"

if (-not $NoBuild) {
    Write-Host "▸ 编译项目 ($Configuration)..." -ForegroundColor Cyan
    dotnet build $ProjectFile -c $Configuration
    if ($LASTEXITCODE -ne 0) {
        Write-Host "✗ 编译失败" -ForegroundColor Red
        exit 1
    }
}

if (-not (Test-Path $ExePath)) {
    Write-Host "✗ 未找到 $ExePath，请先编译项目" -ForegroundColor Red
    exit 1
}

Write-Host "▸ 启动 ShareOneList ($Configuration)..." -ForegroundColor Cyan
Start-Process $ExePath
