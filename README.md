# 桌面宠物剪贴板 🐾

> A cute desktop pet that doubles as your clipboard manager on macOS and Windows.

Tauri 2 桌面宠物，兼任你的第二剪贴板。

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Platform: macOS 13+ / Windows 10+](https://img.shields.io/badge/Platform-macOS%2013%2B%20%C2%B7%20Windows%2010%2B-lightgrey.svg)](#已知限制)
[![Tests](https://img.shields.io/badge/Tests-Rust%2032%20%C2%B7%20Node%2022%20%C2%B7%20%E5%A5%91%E7%BA%A6%2073-brightgreen.svg)](#自动化测试)

## 下载安装（Windows）

到 [Releases](https://github.com/yppppppHey/pet-clipboard/releases/latest) 下载 `Desktop.Pet.Clipboard_0.1.1_x64-setup.exe`，双击安装（按当前用户安装，不需要管理员权限）。
本地打包产出的文件名是 `Desktop Pet Clipboard_0.1.1_x64-setup.exe`，上传到 GitHub Release 时空格会被替换成点号，二者是同一个文件。

安装包没有代码签名，SmartScreen 可能提示「Windows 已保护你的电脑」：点「更多信息」→「仍要运行」。
macOS 暂不提供安装包，从源码构建即可（见下）。

## 快速开始

仓库自带全部素材与依赖锁定（`Cargo.lock`），无任何外部服务依赖：

```bash
git clone https://github.com/yppppppHey/pet-clipboard.git
cd pet-clipboard
cargo run --manifest-path src-tauri/Cargo.toml --locked
```

macOS 首次运行需要授予辅助功能权限，步骤见下方「首次运行」。
Windows 用同一份代码（`#[cfg]` 双平台，需要 Rust MSVC 工具链与 WebView2 运行时），不需要任何授权：

```powershell
git clone https://github.com/yppppppHey/pet-clipboard.git
cd pet-clipboard
cargo run --manifest-path src-tauri/Cargo.toml --locked
```

## 已知限制

- 全局按键旁听已实现于 macOS（CGEventTap）与 Windows（低级钩子）；Linux 尚未适配
- macOS 无开发者签名的应用无法穿透原生全屏 Space 浮窗，全屏场景请使用菜单栏入口（置顶桌宠除外，见下文）
- Windows 上粘贴唤起即临时置顶，可盖住无边框全屏、F11、最大化窗口；但**独占全屏**（DirectX 独占，多见于游戏/反作弊客户端）由系统合成器接管，任何置顶窗口都无法覆盖
- 以管理员权限运行的窗口内的 Ctrl+C/Ctrl+V 无法被普通权限的钩子旁听（UIPI 隔离），不影响该程序自身的复制粘贴
- 剪贴板数据为明文本地存储，路径按平台区分：macOS `~/.desktop-pet-clipboard/store.json`，Windows `%LOCALAPPDATA%\desktop-pet-clipboard\store.json`，请勿收集密码等敏感内容

## 功能

- **双窗口**：透明小宠物（本体可拖拽）+ 默认隐藏的剪贴板管理面板
- **桌宠置顶**：启动时默认不置顶；右上角图钉切换，粉色表示已置顶。取消后立即降低为普通窗口，同时移除跨应用全屏浮层标记；所有场景保持显示，不因点击图钉自身而隐藏；后续粘贴可再次临时前置，但不会重新置顶
- **Tab 1 永久存**：自己添加 key/value，上限 100 条，模糊搜索（按匹配度排序），点击复制，hover 删除，持久化在 `~/.desktop-pet-clipboard/store.json`
- **Tab 2 最近复制**：最多 10 条，去重置顶
- **Cmd/Ctrl+C 旁听**（不拦截不吞键）：在任意 App 复制 → 自动进最近复制（250ms 去抖，忽略自动连发）
- **Cmd/Ctrl+V 旁听**：V 键松开后等待 120ms，再让桌宠临时前置并跳动；无论是否置顶都会响应。不请求键盘焦点、不改变窗口层级或图钉状态、不弹面板、不写回剪贴板。未置顶时仍是普通窗口；点击桌宠以外任意位置后隐藏。Windows 上唤起即临时置顶（显示前后各压一次 `HWND_TOPMOST`，前台全屏期间每 750ms 重申），因此全屏应用里也能浮在最前；隐藏、打开面板或取消固定时会撤销这次临时置顶
- **统一点击隐藏**：桌面、普通窗口、最大化和原生全屏规则一致，未置顶时点击桌宠以外任意位置即调用原生 `orderOut:` 隐藏并检查 `isVisible`。唯一例外是桌宠自身（含图钉和剪贴板按钮）；点击独立剪贴板面板也会隐藏桌宠，但不关闭面板、不吞点击。置顶状态优先：置顶后不受外部点击隐藏影响，取消置顶后恢复统一点击隐藏；再次粘贴或菜单「显示桌宠」可找回。
- **全屏/桌面切换**：进入全屏时隐藏未置顶桌宠；退出全屏或还原窗口不自动找回已隐藏的桌宠，避免外部点击后又自行弹出。Windows 区分两种收起：因前台铺满屏幕而自动收起的，离开满屏后自动恢复；用户点击收起的仍不复活。手动置顶不受切换隐藏和外部点击隐藏影响。
- **面板入口**：仅点击桌宠右下角 📋 打开/收起面板；右上角 × 隐藏面板，不退出应用、不清空数据或未提交的输入；启动、复制、粘贴、权限通知和菜单栏均不会打开面板
- **面板焦点**：面板是普通非置顶窗口，手动打开时聚焦；失焦不再额外重排窗口，避免关闭点击被焦点切换打断。支持未激活时首次点击直接响应，关闭按钮独立于拖拽区域，在按下时关闭；桌宠图钉不控制面板。Windows 上若前台本身就是置顶窗口（全屏播放器等），打开面板时会临时置顶面板并撤掉桌宠的临时置顶，保证面板不被盖住。
- **多屏适配**：粘贴时宠物随鼠标所在屏幕浮现，窗口避开 Dock 和菜单栏；同屏保留拖动后的位置
- **点击条目** → 写回系统剪贴板，去任何地方粘贴
- **菜单栏**：「显示桌宠」可找回被其他窗口遮住的桌宠，不改变其置顶选择，也不打开面板；「退出桌宠」完全退出应用

## 首次运行（macOS）

1. 安装 Rust 工具链与 Xcode Command Line Tools；项目已附带宠物素材，无需重新生成。
2. 在项目目录构建运行：
   ```bash
   cargo run --manifest-path src-tauri/Cargo.toml --locked
   ```
3. **辅助功能授权（监听快捷键必需）**：点击宠物气泡或面板中的权限提示，打开
   系统设置 → 隐私与安全性 → 辅助功能 → 勾选运行进程所属 App（终端或已打包的 App）。
   后台每 2 秒重试启用监听，无需因为首次未授权而重启应用；未授权时仍可手动添加、搜索和复制。

全局监听目前仅实现于 macOS。应用使用 Accessory/LSUIElement 菜单栏代理模式；桌宠启用置顶或临时前置时，macOS 13+ 设置 `CanJoinAllApplications` 以加入其他应用全屏 Space。展示资格与层级分开：粘贴不提升未置顶桌宠的窗口层级，不抢输入焦点。未置顶时所有屏幕模式统一执行外部点击隐藏，只有点击桌宠自身例外；已置顶时外部点击不隐藏，独立面板不在例外范围内。隐藏通过 `orderOut:` 实施并读取 `isVisible` 验证；再次粘贴或菜单「显示桌宠」可找回。
跨应用全屏显示仍须在实际桌面环境验收，不能用单元测试或「置顶」属性代替验证；不会为了显示宠物而激活应用或模拟按键。

## 更新后必须退出旧进程

替换可执行文件不会替换已经运行的进程，再次双击也可能只是唤起旧实例。

- macOS：先通过菜单栏右键 →「退出桌宠」，确认退出后再打开项目中的应用。旧版没有退出菜单时，可在「活动监视器」中搜索 `desktop-pet-clipboard`，退出同名旧进程后再打开新版。
- Windows：托盘图标右键 →「退出桌宠」；或在任务管理器结束 `desktop-pet-clipboard.exe`。安装新版本之前也要先退出，否则安装目录里的 exe 会被占用。

托盘菜单第一项显示构建标识（当前为「v0.1.1 · 全屏置顶修复 · 1028-1」），可用来确认正在运行的是新进程。标准错误输出隐藏结果，不包含剪贴板内容。

如果当前粘贴受到影响，先完全退出桌宠以隔离问题；不要通过反复复制或强制激活桌宠来恢复。

## 打包

```bash
./make_app.sh          # 默认 release：thin LTO、单代码生成单元、移除符号
./make_app.sh --debug  # 开发用快速打包
```

脚本先构建、暂存、签名并验证，全部成功后才替换项目中的 `Desktop Pet Clipboard.app`；构建或签名失败不会删除旧应用。支持路径中包含空格。
前端资源通过 `custom-protocol` 嵌入二进制，无需另行部署。ad-hoc 签名不是固定的开发者身份，重新构建后 macOS 可能要求重新授予辅助功能权限。

### Windows 打包

```powershell
powershell -ExecutionPolicy Bypass -File make_app.ps1            # 全量测试 -> 打包 -> 复制到 dist\
powershell -ExecutionPolicy Bypass -File make_app.ps1 -SkipTests # 跳过测试快速打包
```

脚本按顺序跑 Rust 单测、前端测试与平台契约测试，全部通过后才构建 NSIS 安装包；任一失败立即中止，`dist\` 保留上一次的产物。
构建步骤优先用 `cargo tauri build`，本机没有安装 `cargo-tauri` 时自动回退到 `npx @tauri-apps/cli@2 build`，两者产出的路径一致（`src-tauri\target\release\bundle\nsis\`）。

也可以直接用 Tauri CLI 打包：

```powershell
npx --yes @tauri-apps/cli@2 build
```

## 可靠性与隐私

- 存储格式和路径与旧版兼容。使用同目录临时文件、同步写入和原子替换；保存失败返回错误，不更新内存中的数据。
- 数据文件损坏或不可读时停止启动并在标准错误中说明原因，不再静默初始化空数据覆盖原文件。请先备份再修复。
- 连续复制由单个后台工作线程处理，事件回调不读剪贴板、不写磁盘；重复队首记录不会再次写盘或刷新界面。
- 单条新内容最多 1 MB、名称最多 1 KB；超过大小限制的自动复制记录会跳过。永久存上限 100 条，最近复制上限 10 条。
- 新写入的数据文件在 macOS 上仅当前用户可读写（`0600`），新建存储目录为 `0700`。**数据仍是明文，未做加密**，请避免收集密码等敏感内容。
- 前端启用 CSP，剪贴板内容仅以纯文本渲染；搜索去抖并丢弃过期请求结果，保存/复制/删除失败会显示提示。
- 仅监听普通 Cmd/Ctrl+C、Cmd/Ctrl+V，不记录鼠标菜单中的复制，也不监听带 Shift/Option 的组合键。
- 鼠标按下同样使用 listen-only 旁听，只传递目标窗口编号与事件发生时的坐标以判断是否点击桌宠之外；可见窗口区域保护不依赖可能缺失的事件窗口编号，不吞掉或模拟点击，不持久化位置、点击历史或页面内容。鼠标与粘贴事件顺序处理，粘贴延迟期间点击会取消待展示，避免隐藏后又自行弹出。

## 自动化测试

无需安装前端依赖；需要 Rust、Node.js 18+ 与 Python 3。

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked
node --test tests/frontend.test.cjs
python3 -m unittest discover -s tests -p 'test_*.py' -v
```

Windows 上用同一组命令（契约测试换成 Windows 版）：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked
node --test tests/frontend.test.cjs
python -m unittest discover -s tests -p "test_*_windows.py" -v
```

当前规模：Rust 单元测试 **32** 项、前端逻辑 **22** 项、平台契约 **73** 项，全部为纯本地静态/逻辑测试。

测试覆盖持久化与失败回滚、Unicode、去重与容量、快捷键识别、连续粘贴时序、跨应用全屏标记、手动面板入口、关闭保留输入、桌宠置顶切换、搜索竞态、错误提示、重复提交、权限事件和打包失败恢复。测试使用临时目录与模拟接口，不读写真实剪贴板数据。
`test_native_contract.py` 仅防止原生调用路径和配置回归，不启动 WindowServer，也不证明全屏和焦点行为已通过实测。
macOS 辅助功能、原生拖拽、实际粘贴焦点以及全屏/多屏行为仍需按以下清单手动验收。

## 满屏识别

这里的“全屏”同时包括 macOS 原生全屏 Space，以及 Chrome 等前台窗口铺满某块显示器工作区的情况（保留标签栏/地址栏也算）。系统展示标志仅作一路判断，另一路读取 WindowServer 的窗口几何元数据，按前台应用最前面的普通窗口与各显示器工作区比较，支持菜单栏、Dock 和负坐标副屏；不捕获屏幕像素、不读取窗口标题。半屏分屏和普通大小窗口不会按满屏处理。

每 750ms 检查一次模式变化，仅进入满屏时自动隐藏未置顶桌宠，不在退出满屏时自动恢复。外部点击隐藏与满屏识别完全独立，先检查置顶状态：已置顶不隐藏；未置顶时不要求曾粘贴唤起，只有点击桌宠自身例外。

Windows 侧的检测同源但只做几何比较：`GetForegroundWindow` 取前台窗口，`MonitorFromWindow` + `GetMonitorInfoW` 取该屏工作区 `rcWork`，`GetWindowRect` 与之按 10px 容差比较，因此 F11 全屏与最大化都会命中；不截屏、不读标题、不读进程名。

与 macOS 的关键差别是**粘贴唤起即临时置顶**：唤起不再依赖满屏检测来决定层级，检测只用于「进入满屏自动收起」与「离开满屏自动恢复上一次被收起的桌宠」。这样即使检测漏判（全屏窗口在另一块屏、置顶型全屏应用、焦点瞬时漂移），粘贴后桌宠也一定压在最前；750ms 观察线程每轮再重申一次临时置顶，防止全屏应用把自己抢回最前。

## 验收清单

- [ ] 启动时桌宠图钉为未置顶，位于普通应用窗口后面；面板隐藏；复制、粘贴、点击菜单栏图标或权限变化均不打开面板
- [ ] 桌宠可拖拽；仅点击右下角 📋 → 面板显示/隐藏；标题栏可拖动面板
- [ ] 面板打开后点击其他应用，再直接按面板右上角 × 一次 → 面板隐藏；再次通过 📋 打开，保存内容和未提交输入仍在
- [ ] 任意 App 里 Cmd/Ctrl+C 三次（间隔超过 250ms）→ 最近复制出现 3 条
- [ ] 永久存添加 key=邮箱 → 搜索“邮”能出来 → 重启 App 仍在
- [ ] 面板点某条 → 别处 Cmd+V 出同样内容
- [ ] 默认不置顶时按 Cmd/Ctrl+V → 桌宠临时浮到普通应用窗口前面并跳动，图钉保持未置顶；原 App 收到粘贴且焦点不变，面板不打开
- [ ] 桌面、普通窗口、半屏、最大化、原生全屏中：粘贴显示桌宠，点击桌宠不隐藏；点击桌面或其他页面立即隐藏，再粘贴可找回
- [ ] 手动打开面板后点别的窗口 → 正常切换，不强制重排窗口
- [ ] 在普通编辑器中连续粘贴 10 次；再按住 Cmd，反复按 V 10 次，原 App 均正常收到粘贴
- [ ] 未置顶和已置顶两种状态下，切换到另一个应用的原生全屏 Space（不是仅最大化），重复粘贴；宠物可见，不退出全屏、不改变输入目标，图钉状态不变
- [ ] 未置顶时进入全屏默认无桌宠；Cmd/Ctrl+V 显示后，点击全屏页面任意位置 → 桌宠消失，页面正常响应该点击；再粘贴可再次显示
- [ ] Chrome 铺满屏幕时：默认隐藏，粘贴显示，点击网页隐藏；最大化后再还原不自动找回已隐藏桌宠
- [ ] 点击独立剪贴板面板：桌宠隐藏，但面板不关闭，条目复制、搜索和关闭按钮仍正常响应
- [ ] 在普通桌面粘贴叫出桌宠后，直接用键盘切到其他全屏 Space → 默认不显示桌宠；只有在新页面再次粘贴才临时出现
- [ ] 点击桌宠本体、📋、图钉：可拖动、开关面板和置顶；不因内部点击意外退场
- [ ] 粘贴后在 120ms 内立即点击页面 → 桌宠不会在点击后延迟弹出；等待后再粘贴仍可正常显示
- [ ] 右键菜单栏图标 →「退出桌宠」，菜单栏图标和宠物均消失；再打开应用验证为新进程
- [ ] 开启置顶后点击其他页面、独立面板和切换全屏均不隐藏桌宠；取消置顶后点击外部立即隐藏，粘贴可再次找回
- [ ] 取消置顶后，菜单栏 →「显示桌宠」能找回桌宠，图钉仍是未置顶，剪贴板面板仍隐藏
- [ ] 首次未授权时提示可见、可打开设置；授权后监听自动启用
- [ ] 在不同缩放比例/同名显示器之间移动鼠标，宠物出现在正确屏幕且不被 Dock 遮挡

### Windows 验收清单

- [ ] 双击安装包即可完成安装（按当前用户装、不弹 UAC）；托盘菜单第一项显示 `v0.1.1 · 全屏置顶修复 · 1028-1`
- [ ] 打开剪贴板面板：背景是不透明纯色卡片，看不到后面的窗口内容（圆角外侧仍透出桌面）
- [ ] 浏览器 F11 全屏、视频播放器全屏、窗口最大化时按 Ctrl+V：桌宠出现在全屏画面之上；点击全屏区域后桌宠消失
- [ ] 启动时前台是最大化窗口 → 桌宠自动收起；点一下桌面或恢复窗口大小 → 桌宠自动回来
- [ ] 点击桌宠收起后，恢复窗口大小不会让它自己弹回来；再按 Ctrl+V 或托盘「显示桌宠」可找回
- [ ] 置顶（图钉）后外部点击与全屏切换都不隐藏；取消置顶后恢复上述规则
- [ ] 独占全屏游戏单独验证：属已知限制，任何置顶窗口都盖不住，不算回归

## 目录结构

```
frontend/          # pet.html / panel.html + 独立 JS（零构建）
frontend/assets/pet.png   # 宠物透明素材
src-tauri/         # Rust 后端：store / hotkey / 桌宠置顶与面板显隐
  src/main.rs           # 共享业务主体 + 双平台接线
  src/hotkey.rs         # 共享时序状态机 + 各平台事件源（CGEventTap / 低级钩子）
  src/macos_window.rs   # macOS：AppKit / Space
  src/windows_window.rs # Windows：Win32 无激活显示 / HWND / 临时置顶
  src/virtual_desktop.rs# Windows：跨虚拟桌面固定（失败自动降级）
tests/             # 零额外依赖的前端逻辑、原生契约与打包回归测试
.github/workflows/windows.yml  # Windows CI：测试 -> NSIS 打包 -> tag 时附到 Release
```

素材处理：原图 `assets/material.png`，经 AI 图像工具（Nano Banana Pro）高清重绘得到候选 `aigc_1.png`（采用）/ `aigc_2.png`（备选），再用 Pillow 去除白色背景生成透明素材 `pet.png`。原图与全部候选图均为项目自带文件，无外部服务依赖。

## Windows 版

同一份代码通过 `#[cfg]` 双平台编译：前端、存储与业务逻辑共享，平台差异隔离在以下模块。

### 架构（单仓库双平台）

| 共享 | macOS 插件 | Windows 插件 |
| --- | --- | --- |
| `frontend/`、`store.rs`、共享时序状态机（`hotkey.rs` 上半部）、`main.rs` 业务主体 | `macos_window.rs`（AppKit CGEventTap 旁听 + Space） | `windows_window.rs`（Win32 无激活显示/HWND）、`hotkey.rs` 内 `platform`（WH_KEYBOARD_LL / WH_MOUSE_LL 只旁听）、`virtual_desktop.rs`（非官方 COM 跨桌面固定，失败自动降级） |

- 存储：Windows 落在 `%LOCALAPPDATA%\desktop-pet-clipboard\store.json`（`#[cfg]` 分支，macOS 路径不变）。
- 权限：Windows 无「辅助功能」授权，以钩子健康状态代替；监听中断时气泡展示恢复文案（运行时按 UA 选择，Mac 静态文案原样保留）。
- 打包：主 `tauri.conf.json` 保持 macOS `app` 目标不变；NSIS 配置隔离在 `tauri.windows.conf.json` 平台覆盖层。

### Windows 构建与测试

```powershell
# 自动化测试（前端/契约层与平台无关，Rust 单测需 Windows）
cargo test --manifest-path src-tauri/Cargo.toml
node --test tests/frontend.test.cjs
python -m unittest discover -s tests -p 'test_*_windows.py' -v

# 一键打包：全量测试 -> cargo tauri build -> NSIS 安装包复制到 dist\（失败保留旧产物）
powershell -ExecutionPolicy Bypass -File make_app.ps1
```

图标由 `gen_icons.py` 从 `frontend/assets/pet.png` 生成（需要 `pip install pillow`）。

连续集成：`.github/workflows/windows.yml` 在真实 `windows-latest` 容器跑全部测试并产出 NSIS 安装包（artifact `windows-nsis-installer`）。

### Windows 已知限制

| 限制 | 说明 | 应对 |
| --- | --- | --- |
| 独占全屏 | DirectX 独占全屏由系统合成器接管，任何置顶窗口都无法覆盖 | 属系统限制；无边框全屏 / F11 / 最大化都已支持 |
| UIPI 隔离 | 普通权限无法旁听"以管理员运行"程序中的 C/V | 默认不提权，不影响其正常使用 |
| 虚拟桌面 | 使用非官方 ImmersiveShell COM，系统更新后可能失效 | 失败自动降级为单桌面，不阻断启动 |
| LL 钩子超时 | 回调过慢会被系统摘除 | 回调只做无锁入队；消息泵退出后 2s 自动重装 |
| 未签名安装包 | SmartScreen 可能提示"未知发布者" | 可选"仍要运行"；后续可补代码签名 |

## 更新日志

### v0.1.1

- **修复**：全屏应用里按 Ctrl+V 后桌宠被盖住。唤起改为「一律临时置顶」，不再依赖全屏检测；`ShowWindow` 前后各压一次 `HWND_TOPMOST`，前台全屏期间每 750ms 重申，防止全屏应用把自己抢回最前。
- **修复**：桌宠因前台铺满屏幕被自动收起后，离开满屏不会恢复。现在区分「被挡住自动收起」与「用户点击收起」，只有前者自动恢复。
- **修复**：取消置顶后，粘贴唤起会把窗口重新压成置顶。临时置顶与固定置顶改为互斥状态。
- **变更**：剪贴板面板背景改为不透明的 `#fffafc`，去掉 `rgba(255,250,252,.88)` 与 `backdrop-filter` 毛玻璃。
- **变更**：Windows 面板在「前台本身是置顶窗口」时也会置顶，并撤掉桌宠的临时置顶，避免面板被盖住。
- **变更**：托盘菜单构建标识改为 `v0.1.1 · 全屏置顶修复 · 1028-1`，用于确认运行的是新进程。
- **测试**：新增 5 条 Windows 契约测试（唤起临时置顶、取消固定、置顶型前台、观察线程重申、自动恢复语义）。

### v0.1.0

- 首个双平台版本：macOS 保持原有能力；新增 Windows 平台（低级钩子旁听、无激活显示、跨虚拟桌面固定、NSIS 安装包与 Windows CI）。
