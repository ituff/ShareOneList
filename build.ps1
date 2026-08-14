#!/usr/bin/env pwsh
<#
.SYNOPSIS
    编译 ShareOneList 项目
.PARAMETER Configuration
    编译配置，Debug 或 Release，默认 Debug
.PARAMETER Clean
    编译前清理输出目录
.EXAMPLE
    .\build.ps1
.EXAMPLE
    .\build.ps1 -Configuration Release
.EXAMPLE
    .\build.ps1 -Clean
#>

param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Debug",

    [switch]$Clean
)

$ErrorActionPreference = "Stop"
$ProjectFile = "SimpleList/SimpleList.csproj"

if ($Clean) {
    Write-Host "▸ 清理项目..." -ForegroundColor Cyan
    dotnet clean $ProjectFile -c $Configuration --verbosity quiet
}

Write-Host "▸ 编译项目 ($Configuration)..." -ForegroundColor Cyan
dotnet build $ProjectFile -c $Configuration

if ($LASTEXITCODE -ne 0) {
    Write-Host "✗ 编译失败" -ForegroundColor Red
    exit 1
}

Write-Host "✓ 编译成功" -ForegroundColor Green
