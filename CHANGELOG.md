# 更新日志

版本号与 `src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 保持一致；每个 Release 附带对应版本的 Windows NSIS 安装包（GitHub 上的资产名为 `Desktop.Pet.Clipboard_<版本>_x64-setup.exe`，本地构建产物为 `Desktop Pet Clipboard_<版本>_x64-setup.exe`，空格被替换成点号，内容相同）。

## [0.1.1] - 2026-11-04

### 修复

- **全屏应用里按 Ctrl+V 后桌宠被盖住**：唤起改为「一律临时置顶」，不再以「检测到前台全屏」为前提。`ShowWindow` 前后各压一次 `HWND_TOPMOST`，抵消显示引起的 Z 序重排；前台全屏期间观察线程每 750ms 重申一次，防止全屏应用把自己抢回最前。
- **离开满屏后桌宠不回来**：区分「被前台铺满屏幕自动收起」与「用户点击收起」，只有前者在离开满屏后自动恢复。
- **取消置顶失效**：粘贴唤起会把窗口重新压成置顶。临时置顶与固定置顶改为互斥状态，取消置顶后不再复用 reveal 路径。

### 变更

- 剪贴板面板背景改为不透明 `#fffafc`，移除 `rgba(255, 250, 252, .88)` 与 `backdrop-filter` 毛玻璃；圆角与边框保留。
- Windows：打开面板时若前台本身是置顶窗口（全屏播放器等），临时置顶面板并撤掉桌宠的临时置顶，避免面板被盖住。
- 托盘菜单构建标识改为 `v0.1.1 · 全屏置顶修复 · 1028-1`，用于确认运行的是新进程。
- `make_app.ps1`：未安装 `cargo-tauri` 时自动回退到 `npx @tauri-apps/cli@2 build`。
- 文档：README 增加 Windows 下载安装、Windows 打包、Windows 验收清单与更新日志；`系统分析-Windows版.md` 同步临时置顶机制、取消置顶流程与存储路径（`%LOCALAPPDATA%`）。

### 测试

- Rust 单元测试 32 项、前端逻辑 22 项、Windows 平台契约 73 项全部通过。
- 新增 5 条 Windows 契约测试：唤起临时置顶、隐藏/取消固定清理、前台置顶识别、观察线程重申、自动恢复语义。

### 已知限制

- 独占全屏（DirectX 独占，多见于游戏/反作弊客户端）由系统合成器接管，任何置顶窗口都无法覆盖。
- 普通权限无法旁听「以管理员运行」的窗口内的 Ctrl+C/Ctrl+V（UIPI 隔离）。
- 安装包未做代码签名，SmartScreen 可能提示「未知发布者」。

## [0.1.0] - 2026-11-04

### 新增

- 首个双平台版本：macOS 保持原有能力（CGEventTap 旁听、全屏 Space 固定、ad-hoc 打包脚本）。
- 新增 Windows 平台：低级键盘/鼠标钩子只旁听、`SW_SHOWNOACTIVATE` 无激活显示、Hwnd 级置顶与命中判定、跨虚拟桌面固定（失败自动降级）、NSIS 安装包与 Windows CI。
- 前端、存储与业务逻辑双平台共享，平台差异隔离在 `macos_window.rs` / `windows_window.rs` / `virtual_desktop.rs` 与 `hotkey.rs` 的平台子模块。
