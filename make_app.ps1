#requires -Version 5.1
<#
.SYNOPSIS
  桌面宠物剪贴板（Windows 版）打包脚本。

.DESCRIPTION
  流程与系分 §11.2 对齐：
  1. 先运行全量自动化测试（cargo test / node --test / python 契约与打包回归），
     任一失败立即中止，不进入构建；
  2. `tauri build` 产出 NSIS 安装包（--debug 走 Debug 分支；本机没有 cargo-tauri
     时自动回退到 `npx @tauri-apps/cli@2`，两者产物路径一致）；
  3. 构建成功且确认产物存在后，才复制到 dist 目录。
  dist 目录永不提前清空：任何失败路径都保留上一次的旧产物。

.PARAMETER Mode
  Release | Debug，默认 Release。

.PARAMETER SkipTests
  跳过自动化测试，直接构建（联调时使用）。

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File make_app.ps1
  powershell -ExecutionPolicy Bypass -File make_app.ps1 -Mode Debug -SkipTests
#>
[CmdletBinding()]
param(
    [ValidateSet('Release', 'Debug')]
    [string]$Mode = 'Release',
    [switch]$SkipTests
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = Split-Path -Parent $MyInvocation.MyCommand.Path
$srcTauri = Join-Path $repo 'src-tauri'
$dist = Join-Path $repo 'dist'
$profile = if ($Mode -eq 'Release') { 'release' } else { 'debug' }
$bundleDir = Join-Path $srcTauri (Join-Path 'target' (Join-Path $profile 'bundle\nsis'))

Write-Host "==> 桌面宠物剪贴板打包（$Mode）"

function Assert-LastExitCode([string]$what) {
    if ($LASTEXITCODE -ne 0) {
        throw "$what 失败（exit=$LASTEXITCODE）；dist 未做任何改动"
    }
}

if (-not $SkipTests) {
    Write-Host '==> 阶段 1/2：全量自动化测试'
    Push-Location $repo
    try {
        cargo test --manifest-path src-tauri/Cargo.toml --quiet
        Assert-LastExitCode 'cargo test'
        node --test tests/frontend.test.cjs
        Assert-LastExitCode 'node --test'
        python -m unittest discover -s tests -p 'test_*_windows.py' -v
        Assert-LastExitCode 'python unittest'
    }
    finally {
        Pop-Location
    }
}
else {
    Write-Host '==> 已跳过自动化测试（-SkipTests）'
}

Write-Host "==> 阶段 2/2：tauri build（$Mode）"
$tauriArgs = if ($Mode -eq 'Debug') { @('build', '--debug') } else { @('build') }
Push-Location $repo
try {
    # 优先用本机 cargo 子命令；没有 cargo-tauri 时回退到 npm 分发的同一版本 CLI，
    # 两者产物路径一致（src-tauri\target\<profile>\bundle\nsis）。
    cargo tauri --version *> $null
    if ($LASTEXITCODE -eq 0) {
        cargo tauri @tauriArgs
    }
    else {
        Write-Host '    （未安装 cargo-tauri，回退到 npx @tauri-apps/cli）'
        npx --yes @tauri-apps/cli@2 @tauriArgs
    }
    Assert-LastExitCode "tauri build（$Mode）"
}
finally {
    Pop-Location
}

# 只有构建成功才查产物；找不到产物视为失败，dist 保持原样。
$installers = @(Get-ChildItem -Path $bundleDir -Filter '*.exe' -ErrorAction SilentlyContinue |
    Sort-Object -Property LastWriteTime -Descending)
if ($installers.Count -eq 0) {
    throw "构建成功但没有找到 NSIS 产物：$bundleDir；dist 未做任何改动"
}

New-Item -ItemType Directory -Force -Path $dist | Out-Null
foreach ($installer in $installers) {
    $dest = Join-Path $dist $installer.Name
    Copy-Item -LiteralPath $installer.FullName -Destination $dest -Force -ErrorAction Stop
    Write-Host "==> 已复制产物：$dest"
}

Write-Host "==> 打包完成（$Mode）：$($installers[0].Name)"
