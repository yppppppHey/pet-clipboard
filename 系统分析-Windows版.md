# 桌面宠物剪贴板 · Windows 版系统分析（系分）

> 版本：v0.1（评审稿）
> 日期：2026-11-03
> 基线仓库：`yppppppHey/pet-clipboard`（Tauri 2 + Rust，当前仅适配 macOS 13+）
> 目标：在保持功能与交互完全对齐的前提下，产出 Windows 版本；前端与图片原型完整复用，技术栈沿用 Tauri 2 + Rust。

---

## 1. 背景与目标

### 1.1 产品定位
"桌面宠物剪贴板"是一个常驻桌面的透明小宠物（红色星星苹果），同时兼任第二剪贴板：
- 双窗口：可拖拽的透明桌宠（168×140）+ 默认隐藏的剪贴板面板（380×520）。
- 两个 Tab：**永久存**（key/value，上限 100，模糊搜索，点击复制，hover 删除）与 **最近复制**（上限 10，去重置顶）。
- 全局旁听 Ctrl/⌘+C、Ctrl/⌘+V（只旁听、不拦截、不吞键），支持点击外部隐藏、置顶切换、多屏、托盘菜单。

### 1.2 本次目标
1. 复用 Mac 版全部前端代码与图片素材（含原型），不重做视觉。
2. 沿用 Tauri 2 + Rust 技术栈，通过**平台抽象层**补齐 Windows 原生能力。
3. 功能、交互、数据格式与 Mac 版完全对齐，v1 不新增、不删减功能。
4. 建立可在 Windows 上运行的全量自动化测试 + 真实桌面手工验收，执行"开发 → 全量测试 → 修复 → 回测"闭环。

### 1.3 非目标（v1 不做）
- 不做图片/文件等富文本剪贴板（仅纯文本，与 Mac 版一致）。
- 不做数据加密、云同步、多语言（仅简体中文 + LTR）。
- 不做深色模式。

---

## 2. 现状分析（Mac 版架构）

### 2.1 技术栈
| 层 | 技术 | 说明 |
| --- | --- | --- |
| 前端 | 原生 HTML/CSS/JS（零构建） | `pet.*`、`panel.*`，通过 `window.__TAURI__`（`withGlobalTauri`）调用后端 |
| 应用框架 | Tauri 2 | 双窗口、系统托盘（tray-icon）、CSP、WebView |
| 后端语言 | Rust（edition 2021） | 命令处理、状态管理、持久化、全局监听 |
| 剪贴板 | `arboard 3` | 跨平台剪贴板读写 |
| 模糊搜索 | `fuzzy-matcher 0.3`（SkimMatcherV2） | 永久存 key 匹配打分 |
| 目录 | `dirs 5` | 定位用户主目录 |
| 平台原生（Mac） | ApplicationServices / AppKit / CoreFoundation / CoreGraphics（libobjc FFI） | CGEventTap、窗口层级、Space、全屏检测 |

### 2.2 模块构成
```
frontend/
  pet.html / pet.js        # 桌宠窗口（拖拽、置顶按钮、面板入口、权限气泡）
  panel.html / panel.js    # 面板窗口（Tab、搜索、列表、添加表单、toast）
  assets/pet.png           # 透明宠物素材（红色星星苹果）
src-tauri/
  src/main.rs              # 入口：Tauri 命令、状态、窗口摆放、托盘、线程编排
  src/store.rs             # 跨平台：JSON 持久化、容量/大小校验、模糊搜索、原子写
  src/hotkey.rs            # Mac：CGEventTap 全局键/鼠旁听 + 工作线程（含去抖时序）
  src/macos_window.rs      # Mac：AppKit/objc 窗口层级、显隐、命中、全屏、Space 通知
  tauri.conf.json          # 双窗口、CSP、bundle 配置
tests/
  frontend.test.cjs        # Node 前端逻辑测试（平台无关）
  test_native_contract.py  # Python 原生契约（强 Mac 绑定）
  test_packaging.py        # Python 打包脚本回归（make_app.sh）
```

### 2.3 关键运行时机制
- **通道 + 工作线程**：事件回调只做"入队"，不读剪贴板、不写盘；后端两个 worker 分别处理复制与粘贴/点击。
- **复制去抖**：250ms 内连续事件合并，停顿后再读一次剪贴板。
- **粘贴延迟**：V 键松开后等 120ms，再让桌宠临时前置并跳动；期间若有点击则取消待展示，避免隐藏后又自行弹出。
- **被动展示**：显示桌宠不抢焦点、不改变窗口层级（Mac 用 `orderFrontRegardless`，非 `makeKeyAndOrderFront`）。
- **置顶优先**：置顶后不受外部点击与 Space 切换影响；未置顶点击外部即原生隐藏并校验可见性。

---

## 3. 复用评估（核心结论）

> 结论：**约 80% 代码可直接复用**。前端 4 个文件、图片素材、`store.rs`、`main.rs` 中的命令与业务逻辑全部平台无关；仅"全局键/鼠监听"和"窗口/全屏原生操作"两块需要 Windows 实现，且 Mac 版已预留 `cfg(not(target_os="macos"))` 桩，改造成本可控。

| 模块 | 复用判断 | 说明 |
| --- | --- | --- |
| `frontend/pet.html` `pet.js` | ✅ 完全复用 | 仅字体栈与"辅助功能"文案需按 Windows 语义微调（见 §7.4） |
| `frontend/panel.html` `panel.js` | ✅ 完全复用 | 同上；`backdrop-filter` 在 WebView2（Chromium）支持 |
| `frontend/assets/pet.png` 等素材 | ✅ 完全复用 | 透明 PNG 直接使用，无需重绘 |
| `store.rs` | ✅ 完全复用 | 已用 `#[cfg(unix)]` 隔离权限位；Windows 走 NTFS ACL |
| `main.rs` 命令/状态/窗口摆放/托盘 | ✅ 复用（局部 cfg 改造） | 业务逻辑平台无关，仅平台调用点切换 |
| `hotkey.rs` 时序与状态机 | 🔶 接口复用、平台实现重写 | 复用 `PresentationState`/去抖时序；新增 Windows LL 钩子事件源 |
| `macos_window.rs` | ⛔ 不适用 | 新增对等的 `windows_window.rs`（Win32） |
| `tauri.conf.json` | 🔶 小改 | bundle 目标改 `nsis/msi`，新增 `icon.ico`；窗口配置不变 |
| `Cargo.toml` | 🔶 小改 | 新增 `[target.'cfg(windows)']` 依赖 `windows` |
| `make_app.sh` / `test_packaging.py` | ⛔ 不适用 | 新增 `make_app.ps1` 及对应打包测试 |
| `frontend.test.cjs` | ✅ 完全复用 | 前端原样复用，测试原样通过 |
| `test_native_contract.py` | ⛔ Mac 专属 | 新增 `test_native_contract_windows.py`，保持同等行为不变量 |

---

## 4. Mac ↔ Windows 关键技术映射

| 能力 | macOS 实现 | Windows 实现（拟） |
| --- | --- | --- |
| 全局键盘旁听 | `CGEventTapCreate`（listen-only） | `SetWindowsHookExW(WH_KEYBOARD_LL)` + 消息循环，始终 `CallNextHookEx` |
| 全局鼠标旁听 | CGEventTap（鼠标 down 事件） | `WH_MOUSE_LL`（`WM_LBUTTONDOWN` 等） |
| 不吞键/不拦截 | LISTEN_ONLY，原样返回 event | 回调始终 `CallNextHookEx` 并返回其结果 |
| 显示不抢焦点 | `orderFrontRegardless`（非 makeKey） | `ShowWindow(SW_SHOWNOACTIVATE)` / `SetWindowPos(SWP_NOACTIVATE)` |
| 隐藏 | `orderOut:` + `isVisible` 校验 | `ShowWindow(SW_HIDE)` + `IsWindowVisible` 校验 |
| 置顶 | `setLevel:`（3 / 0） | `SetWindowPos(HWND_TOPMOST / HWND_NOTOPMOST, SWP_NOACTIVATE)` |
| 点击命中 | `frame` + 坐标翻转 + contains | `GetWindowRect` + `PtInRect`（先 `IsWindowVisible`） |
| 窗口标识 | `windowNumber` | `HWND`（as isize） |
| 全屏/铺满检测 | `currentSystemPresentationOptions` + `CGWindowList` 几何 | `GetForegroundWindow` + `MonitorFromWindow` + `GetMonitorInfo(rcWork)` + `GetWindowRect` 几何比较 |
| Space/虚拟桌面通知 | `NSWorkspaceActiveSpaceDidChangeNotification` | 默认单桌面绑定（可选非官方 COM，见 §9 风险） |
| 常驻模式 | `ActivationPolicy::Accessory`（LSUIElement） | `skipTaskbar`（WS_EX_TOOLWINDOW）+ WS_EX_NOACTIVATE（Tauri 已处理） |
| 权限模型 | 辅助功能授权（AXIsProcessTrusted） | 无对应授权；LL 钩子默认可用（UIPI 限制见 §9） |
| 打包 | `.app` + ad-hoc 签名 | NSIS/MSI 安装包（默认不签名，可后续补签名） |

---

## 5. 目标架构

### 5.1 分层与数据流
自上而下：前端层（复用）→ Tauri 桥接（复用）→ Rust 业务核心（复用）→ **平台抽象层（新增 Windows 实现）** → 操作系统。

### 5.2 平台抽象设计原则
- 沿用现有代码风格（自由函数 + `cfg` 模块切换），不引入重型抽象，保证与 Mac 版一致、可双平台同仓编译。
- 两个平台模块对外暴露**完全相同的函数签名**，`main.rs` 与 `hotkey.rs` 只依赖统一接口，不直接写平台 API。

模块划分（拟）：
```
src-tauri/src/
  main.rs                 # 复用；平台调用点改为 cfg 切换
  store.rs                # 原样复用
  hotkey.rs               # 共享时序/状态机 + 平台事件源（mac/windows 两个 platform mod）
  macos_window.rs         # 保留（Mac 构建）
  windows_window.rs       # 新增（Windows 构建，对等实现）
```

平台模块统一接口（两平台各自实现）：
```rust
// window 平台模块
fn configure_all_spaces(hwnd) -> Result<(), String>;
fn show_pet_passively(hwnd, pinned: bool, reveal: bool) -> Result<(), String>;
fn hide_pet(hwnd) -> Result<(), String>;
fn set_pet_pinned(hwnd, pinned: bool) -> Result<(), String>;
fn window_number(hwnd) -> isize;
fn contains_click(hwnd, point: Point) -> bool;
fn system_is_fullscreen() -> bool;
fn observe_space_changes() -> Result<(), String>;
```

---

## 6. 核心模块设计

### 6.1 `windows_window.rs`（新增，对等 macos_window.rs）
依赖官方 `windows` crate（windows-rs），按需启用 feature：
`Win32_Foundation`、`Win32_UI_WindowsAndMessaging`、`Win32_Graphics_Gui`（Gdi）、`Win32_UI_Input_KeyboardAndMouse`。
通过 Tauri 的 `window.hwnd()` 获取 `HWND`。

- **被动显示**：`ShowWindow(hwnd, SW_SHOWNOACTIVATE)`；桌宠 `focusable:false` 已带 `WS_EX_NOACTIVATE`，确保不抢键盘焦点。
- **隐藏**：`ShowWindow(hwnd, SW_HIDE)`，随后 `IsWindowVisible(hwnd)` 必须为 false，否则返回错误（与 Mac `orderOut + isVisible` 同语义）。
- **置顶**：`SetWindowPos(HWND_TOPMOST/HWND_NOTOPMOST, 0,0,0,0, SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE)`。
- **临时置顶（v0.1.1）**：粘贴/快捷键唤起时**一律**先压一次 TOPMOST，不再以全屏检测为前提；`ShowWindow(SW_SHOWNOACTIVATE)` 之后再压一次，抵消显示引起的 Z 序重排。静态原子量 `TEMPORARY_TOPMOST` 与固定置顶互斥，隐藏、打开面板、取消固定时撤销。
- **重申与恢复**：观察线程每轮调用 `reassert_temporary_topmost`（仅在可见且临时置顶时执行），防止全屏/置顶应用把自己抢回最前。
- **前台置顶识别**：`GetWindowLongW(fg, GWL_EXSTYLE) & WS_EX_TOPMOST`，用于面板显示时判断前台是否本身就带置顶（全屏播放器、部分游戏），这类窗口靠普通层级永远盖不住。
- **命中**：先 `IsWindowVisible`，再 `GetWindowRect` 取 `RECT`，`PtInRect(rect, point)`（左/上含、右/下不含，与 Mac contains 一致）。
- **窗口标识**：`HWND.0 as isize`。
- **全屏/铺满检测**（不截屏、不读标题，保持隐私口径）：
  1. `GetForegroundWindow()`；用 `GetWindowThreadProcessId` 排除本进程（打开自己面板不改变后台上下文）。
  2. `MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST)` → `GetMonitorInfoW` 取 `rcWork`（排除任务栏的工作区）。
  3. `GetWindowRect(fg)` 取前台窗口几何，与 `rcWork` 用 10px 容差比较（复用 `covers_work_area` 纯函数逻辑）。
  4. 同时覆盖 F11 原生全屏与最大化窗口（最大化窗口边框略超 `rcWork`，容差吸收）。
- **configure_all_spaces**：Windows 上 Tauri 已根据 `skipTaskbar/focusable` 设置 `WS_EX_TOOLWINDOW/WS_EX_NOACTIVATE`，此函数做兜底校验或 no-op。

### 6.2 `hotkey.rs` Windows 平台实现（新增 platform mod）
- 在独立线程安装两个低级钩子并运行消息泵（`GetMessageW` 循环）；LL 钩子无需注入 DLL，回调在安装线程执行。
- **WH_KEYBOARD_LL（13）**：
  - `wparam` 为 `WM_KEYDOWN/WM_KEYUP`；`lparam` 指向 `KBDLLHOOKSTRUCT { vkCode, flags, .. }`。
  - 自行跟踪按键状态以抑制长按重复（等价 Mac 的 autorepeat 过滤）。
  - 仅识别 `VK_C`(0x43)/`VK_V`(0x56)，要求 Ctrl 按下、Shift/Alt 未按下（`GetAsyncKeyState` 或自维护修饰键状态）。
  - 复制在首次按下触发；粘贴在 V 键松开触发——与 Mac 键位语义一致。
- **WH_MOUSE_LL（14）**：`WM_LBUTTONDOWN/WM_RBUTTONDOWN/WM_MBUTTONDOWN`，取 `MSLLHOOKSTRUCT.pt`（屏幕坐标），向粘贴通道发送 `Click` 事件（同时取消待展示）。
- **只旁听**：回调内只做 channel `try_send`（快速返回，避免触发 `LowLevelHooksTimeout` 被系统摘除），始终 `CallNextHookEx` 并返回其结果。
- **复用共享层**：`PresentationState`（120ms 延迟、点击取消）、复制 250ms 去抖、worker 线程模型全部复用，仅替换事件源与快捷键解析。

### 6.3 `main.rs` 改造点（最小化）
- 模块声明：`#[cfg(windows)] mod windows_window;` 与 Mac 模块互斥。
- 将现有 `cfg(not(target_os="macos"))` 的 `show_pet_passively/set_pet_pinned` 桩替换为调用 `windows_window`。
- 为 `dismiss_pet_on_outside_click` 增加 Windows 孪生（逻辑同 Mac：置顶直接返回 → 命中桌宠豁免 → 否则原生隐藏并校验），仅把 `window_number/contains_click/hide_pet` 换成 Windows 实现。
- 将 `observe_screen_context`（750ms 轮询，仅依赖 `system_is_fullscreen` 与 `present_pet`）放开到 Windows 执行；对应静态原子量同步启用。

### 6.4 前端微调（仅两处，不改结构与逻辑）
1. 字体栈追加 Windows 字体：`"Microsoft YaHei", "Segoe UI"`，保证中文渲染（原 `-apple-system/PingFang` 在 Windows 会回退）。
2. 权限相关文案/气泡：Windows 无"辅助功能"授权，默认隐藏权限气泡与面板权限按钮（具体处理见 §10 待确认问题）。

---

## 7. 关键流程（时序）

### 7.1 复制旁听（Ctrl+C）
```
用户在任意 App 按 Ctrl+C
  → WH_KEYBOARD_LL 回调（只入队，不吞键）
  → copy_worker：250ms 去抖合并，停顿后在主线程 arboard 读剪贴板
  → store.push_recent（去重、容量、原子写）
  → emit "clipboard-added" → 若面板在"最近复制"则刷新
```

### 7.2 粘贴旁听（Ctrl+V）
```
V 键松开
  → WH_KEYBOARD_LL 回调入队 Paste
  → paste_worker：等待 120ms（让原 App 先收到粘贴）
  → 期间无点击 → bring_pet_forward：present_pet（多屏定位 + 被动显示 + 临时置顶）+ emit "v-pressed"（桌宠跳动）
  → 临时置顶不依赖全屏检测：显示前压一次 TOPMOST，显示后再压一次
  → 期间有点击 → 取消待展示，不弹出
```

### 7.3 外部点击隐藏
```
鼠标 down（WH_MOUSE_LL）→ 入队 Click(pt)
  → 置顶：直接返回不隐藏
  → 命中桌宠矩形（PtInRect）：豁免
  → 否则：ShowWindow(SW_HIDE) 并校验 IsWindowVisible=false
```

### 7.4 置顶切换
```
点击图钉 → toggle_pet_pin
  → 先清掉 TEMPORARY_TOPMOST（固定置顶接管层级，避免取消固定又被临时置顶压回最前）
  → SetWindowPos(TOPMOST/NOTOPMOST, SWP_NOACTIVATE) 成功
  → 再更新 PET_PINNED 原子量 → 返回新状态，前端切换粉色样式
  → 取消置顶走 show_no_activate（保持可见），不再复用 reveal 路径
```

---

## 8. 数据模型与存储

- 路径：`%LOCALAPPDATA%\desktop-pet-clipboard\store.json`（`app_data_dir()` 定位；与 Mac 的 `~/.desktop-pet-clipboard` 同构，仅根目录不同）。
- 格式（与 Mac 完全兼容，`#[serde(default)]` 容忍旧结构缺字段）：
```json
{
  "permanent": [{"key": "...", "value": "...", "ts": 0}],
  "recent": [{"value": "...", "ts": 0}]
}
```
- 约束（沿用，不放宽）：单条内容 ≤ 1MB、key ≤ 1KB；永久存 ≤ 100、最近复制 ≤ 10；同目录临时文件 + 同步写入 + 原子替换；保存失败不更新内存。
- Windows 权限：不做 `chmod`（该分支已 `cfg(unix)`），文件继承用户目录 NTFS ACL；数据仍为明文，不收集敏感内容。

---

## 9. 风险与限制

| 风险/限制 | 影响 | 应对 |
| --- | --- | --- |
| **UIPI 隔离** | 普通权限无法捕获"以管理员运行"的程序中的 C/V | 默认不提权；在文档/说明中告知；是否提权见 §10 |
| **LL 钩子超时被摘除** | 回调过慢会被系统静默移除 | 回调只做无锁入队、立即返回；监听线程加健康检查与自动重装（对齐 Mac 2s 重试） |
| **虚拟桌面（Win+Ctrl+D）** | Tauri 的 `visibleOnAllWorkspaces` 仅 Mac 生效；"所有桌面显示"需非官方 COM，版本脆弱 | 默认接受窗口绑定单个虚拟桌面（切换桌面系统自动隐藏）；是否强做跨桌面见 §10 |
| **未签名安装包** | SmartScreen 可能拦截提示 | v1 先不签名、提供说明；后续可补代码签名证书 |
| **Rust 工具链缺失** | 当前机器未装 Rust，无法编译 | 安装 rustup（MSVC）；VS2022 C++ 与 WebView2 已就绪 |
| **crates.io 网络** | 国内拉取依赖可能慢/失败 | 配置 cargo 国内镜像（如 rsproxy/USTC），仅改 `~/.cargo/config.toml` |
| **Windows 图标格式** | 打包需 `.ico`，仓库仅有 PNG | 用 Tauri CLI 由源 PNG 生成全套 Windows 图标 |
| **窗口焦点/全屏实测** | 单测无法替代真实桌面行为（与 Mac 同样 caveat） | 保留自动化契约测试 + 真实桌面手工验收清单 |
| **独占全屏（DirectX）** | 独占全屏由系统合成器接管，任何 TOPMOST 窗口都无法覆盖 | 无边框全屏 / F11 / 最大化已支持；在文档与验收清单中明确标注为系统限制 |

---

## 10. 待确认问题（含推荐方案）

> 以下问题均可直接回复"按推荐"，或逐条给出你的选择。

1. **仓库组织**：推荐**单仓库 + cfg 双平台**（保留 Mac 代码，前端/store 只维护一份，可同时构建双平台）；还是要独立的 Windows 项目？
2. **打包形态**：推荐 **NSIS 安装包**（体积小、现代）；是否还要 MSI 或免安装绿色版（portable）？
3. **权限提示 UI**：Windows 无"辅助功能"授权，推荐**默认隐藏**桌宠气泡与面板权限按钮；是否同意？
4. **虚拟桌面**：推荐 v1 **接受默认单桌面绑定**（稳定、不依赖非官方接口）；是否必须跨所有虚拟桌面显示？
5. **管理员窗口（UIPI）**：推荐**应用不提权**、接受无法捕获管理员程序的 C/V；是否要求以管理员身份运行？
6. **存储路径**：推荐沿用 `~\.desktop-pet-clipboard\store.json`（跨平台一致）；是否改用 `%APPDATA%`？
7. **功能范围**：推荐 v1 **完全对齐 Mac 版、不增不减**；确认？
8. **环境准备**：本机未装 Rust，是否同意我**安装 rustup（MSVC 工具链）并配置 cargo 国内镜像**后再开工？

---

## 11. 测试方案（全量 → 修复 → 回测）

### 11.1 自动化测试分层
| 层 | 命令 | 覆盖 | 复用/新增 |
| --- | --- | --- | --- |
| Rust 单测 | `cargo test --manifest-path src-tauri/Cargo.toml` | store 持久化/回滚/容量/Unicode、Windows 快捷键解析、几何比较纯函数 | 复用 + 新增 Windows 用例 |
| 前端测试 | `node --test tests/frontend.test.cjs` | 搜索竞态、去抖、重复提交、显隐、置顶、错误提示等 | 原样复用 |
| 原生契约 | `python -m unittest tests/test_native_contract_windows.py` | 无激活显示、TOPMOST、SW_HIDE+可见性校验、LL 钩子只旁听/CallNextHookEx、配置回归 | 新增（Mac 版保留） |
| 打包测试 | `python -m unittest tests/test_packaging_windows.py` | make_app.ps1 构建失败保留旧产物、release/debug 分支、参数校验 | 新增 |

### 11.2 构建与回测闭环
1. `cargo build`（debug）→ 修正编译警告与错误。
2. 跑全部四类自动化测试，任一失败即修复并**全量重跑**（不仅跑失败项），直到全绿。
3. `cargo tauri build` 产出安装包，验证打包流程与产物结构。
4. 真实桌面手工验收（§11.3），发现问题改代码后回到步骤 2 全量回测。

### 11.3 真实桌面手工验收清单（由 Mac 版 22 项适配）
- 启动：图钉未置顶、面板隐藏；复制/粘贴/托盘/权限变化均不自动打开面板。
- 桌宠可拖拽；仅 📋 开关面板；标题栏可拖动面板；× 隐藏后输入与数据保留。
- 任意 App 连续 Ctrl+C 三次 → 最近复制出现 3 条；永久存添加/搜索/重启后仍在。
- 面板点条目 → 别处 Ctrl+V 出相同内容。
- 未置顶按 Ctrl+V：桌宠临时前置并跳动、图钉不变、原 App 正常收到粘贴、面板不弹。
- 桌面/普通窗口/最大化/全屏：粘贴显示、点击外部隐藏、再粘贴可找回；点击桌宠自身不隐藏。
- 多显示器：宠物随鼠标所在屏浮现、不被任务栏遮挡、保留拖动位置。
- 托盘："显示桌宠"可找回、"退出桌宠"完全退出；构建版本号可辨识新进程。

### 11.4 完成定义（DoD）
- 四类自动化测试全绿；`cargo tauri build` 成功产出目标安装包。
- §11.3 手工验收全部通过；已知限制（UIPI、虚拟桌面、明文存储）已在说明中明确。
- 前端与图片素材与 Mac 版逐字节一致（除 §6.4 两处微调）。

---

## 12. 开发计划（里程碑）

| 阶段 | 内容 | 产出 |
| --- | --- | --- |
| M0 评审 | 确认本系分与 §10 决策 | 评审结论 |
| M1 环境 | 安装 Rust、配置镜像、生成 Windows 图标、拉取/复用前端与素材 | 可编译空壳 |
| M2 平台层 | 实现 `windows_window.rs` 与 hotkey Windows 事件源，接通 main.rs | 可运行版本 |
| M3 全量测试 | 补齐 Windows 契约/打包测试，全量跑通并修复回测 | 全绿测试报告 |
| M4 打包验收 | NSIS/MSI 打包 + 真实桌面手工验收、修复回测 | 安装包与验收记录 |
| M5 交付 | 源码、安装包、测试与说明文档 | 最终交付物 |
