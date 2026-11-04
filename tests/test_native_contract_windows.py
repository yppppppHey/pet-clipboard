"""Windows 原生契约测试（合并仓库版）。

静态检查 cfg 合并结构中 Windows 侧的行为不变量，同时守护合并纪律：
- Windows 变体一律以 #[cfg(target_os = "windows")] 显式标记并位于 mac 变体之后；
- mac 模块/路径/文案零触碰（本套与 mac 版 test_native_contract.py 互补）。

覆盖：无激活被动显示、原生隐藏可校验、置顶不抢焦点、LL 钩子只旁听、
250ms/2s 常量（120ms 粘贴延迟在共享层 PresentationState）、全屏几何比较、
存储约束与原子写、CSP/标签页配置、NSIS 平台覆盖层、前端 §6.4 Windows 微调。
任何平台可运行：python -m unittest discover -s tests -p "test_*.py"
"""

import json
import re
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SRC = REPO / "src-tauri" / "src"
FRONTEND = REPO / "frontend"


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8-sig")


def fn_body_at(src: str, start: int, name: str) -> str:
    """从 start 起找 fn name( 并配对花括号取函数体。"""
    match = re.compile(rf"\bfn\s+{name}\s*\(").search(src, start)
    assert match, f"源码中没有函数 {name}"
    body_start = src.index("{", match.end())
    depth = 0
    for i in range(body_start, len(src)):
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            depth -= 1
            if depth == 0:
                return src[body_start : i + 1]
    raise AssertionError(f"函数 {name} 的函数体没有闭合")


def windows_body(src: str, name: str) -> str:
    """提取紧跟 #[cfg(target_os = "windows")] 之后同名函数的函数体（合并布局约定）。"""
    for match in re.finditer(rf"\bfn\s+{name}\s*\(", src):
        prefix = src[max(0, match.start() - 100) : match.start()]
        if 'cfg(target_os = "windows")' in prefix:
            body_start = src.index("{", match.end())
            depth = 0
            for i in range(body_start, len(src)):
                if src[i] == "{":
                    depth += 1
                elif src[i] == "}":
                    depth -= 1
                    if depth == 0:
                        return src[body_start : i + 1]
            raise AssertionError(f"window 版 {name} 函数体没有闭合")
    raise AssertionError(f"未找到 {name} 的 #[cfg(windows)] 变体")


def cfg_mod_body(src: str, attr_pattern: str, mod_name: str) -> str:
    """提取 `#[cfg(attr)] mod name { … }` 的 mod 体。"""
    pattern = rf'#\[cfg\({attr_pattern}\)\]\s*\nmod {mod_name} \{{'
    match = re.search(pattern, src)
    assert match, f"源码中没有 #[cfg({attr_pattern})] mod {mod_name}"
    body_start = src.index("{", match.end() - 1)
    depth = 0
    for i in range(body_start, len(src)):
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            depth -= 1
            if depth == 0:
                return src[body_start : i + 1]
    raise AssertionError(f"mod {mod_name} 没有闭合")


class MergeSafetyContract(unittest.TestCase):
    """合并纪律：Mac 侧零触碰的静态守护。"""

    @classmethod
    def setUpClass(cls):
        cls.main = read(SRC / "main.rs")
        cls.hotkey = read(SRC / "hotkey.rs")
        cls.manifest = read(REPO / "src-tauri" / "Cargo.toml")

    def test_mac_platform_modules_stay_untouched(self):
        native = read(SRC / "macos_window.rs")
        # 只禁止携带 windows 平台门/接口注入；局部命名（如 CGWindowList 包装）不在此限
        self.assertNotIn('cfg(target_os = "windows")', native)
        self.assertNotIn("windows::Win32", native)
        self.assertNotIn("CallNextHookEx", native)
        self.assertIn("orderFrontRegardless", native)

    def test_mac_wiring_and_tray_are_preserved(self):
        self.assertIn('#[cfg(target_os = "macos")]\nmod macos_window;', self.main)
        self.assertIn("set_activation_policy(tauri::ActivationPolicy::Accessory)", self.main)
        self.assertIn("activate_ignoring_other_apps(false)", self.main)
        self.assertIn("macos_window::observe_space_changes()?", self.main)
        self.assertIn("1028-1", self.main, "托盘版本标识（Mac 契约锚点）保留")

    def test_mac_shared_hotkey_layer_is_reused_not_duplicated(self):
        # 共享层 cfg 放宽到 windows，而非复制出第二份状态机；
        # cfg 与 struct 之间允许存在 #[derive] 等属性行（#\[ 为字面 [，其后才是字符类）
        for ty in ("Point", "PresentationState"):
            self.assertRegex(
                self.hotkey,
                r'#\[cfg\(any\(target_os = "macos", target_os = "windows", test\)\)\]\n'
                rf'(?:#\[[^\n]*\n)*[^\n]*\bstruct {ty}\b',
            )
        # mac 的快捷键解析器保持 macos+test 门（VK 解析是 windows 模块内部实现）
        self.assertRegex(
            self.hotkey,
            r'#\[cfg\(any\(target_os = "macos", test\)\)\]\s*\nfn shortcut\(',
        )

    def test_mac_storage_path_is_preserved(self):
        store = read(SRC / "store.rs")
        self.assertIn("#[cfg(not(target_os = \"windows\"))]", store)
        self.assertIn('join(".desktop-pet-clipboard/store.json")', store)
        self.assertIn("#[cfg(unix)]", store, "mac 目录/文件权限位必须保留")

    def test_mac_custom_protocol_feature_is_preserved(self):
        self.assertIn('default = ["custom-protocol"]', self.manifest)
        self.assertIn('custom-protocol = ["tauri/custom-protocol"]', self.manifest)

    def test_mac_bundle_config_is_preserved_and_windows_uses_overlay(self):
        main_conf = json.loads(read(REPO / "src-tauri" / "tauri.conf.json"))
        self.assertEqual(main_conf["bundle"]["targets"], "app", "主配置保持 macOS app 目标")
        overlay = json.loads(read(REPO / "src-tauri" / "tauri.windows.conf.json"))
        self.assertEqual(overlay["bundle"]["targets"], ["nsis"])

    def test_mac_copy_writing_is_kept_verbatim(self):
        html = read(FRONTEND / "pet.html")
        self.assertIn("打开辅助功能设置", html, "HTML 静态行保留 Mac 授权文案")
        self.assertIn("「辅助功能」，开启复制和粘贴监听", html)
        panel = read(FRONTEND / "panel.html")
        self.assertIn("快捷键监听未启用，点击打开辅助功能设置", panel)


class WindowsWindowContract(unittest.TestCase):
    """windows_window.rs：窗口原生操作契约。"""

    @classmethod
    def setUpClass(cls):
        cls.src = read(SRC / "windows_window.rs")

    def test_passive_show_never_activates(self):
        self.assertIn("SW_SHOWNOACTIVATE", self.src)
        self.assertIsNone(re.search(r"\bSW_SHOW\b", self.src))
        self.assertNotIn("SetForegroundWindow", self.src)

    def test_showwindow_flags_limited_to_passive_show_and_hide(self):
        flags = re.findall(r"ShowWindow\([^,]+,\s*(SW_[A-Z]+)\)", self.src)
        self.assertEqual(set(flags), {"SW_SHOWNOACTIVATE", "SW_HIDE"})

    def test_show_and_hide_both_verify_visibility(self):
        self.assertIn("仍不可见", self.src)
        self.assertIn("仍然可见", self.src)

    def test_pin_uses_topmost_without_activation(self):
        self.assertIn("HWND_TOPMOST", self.src)
        self.assertIn("HWND_NOTOPMOST", self.src)
        self.assertIn("SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE", self.src)

    def test_click_hit_tests_visibility_first_then_rect(self):
        body = fn_body_at(self.src, 0, "contains_click")
        self.assertLess(body.index("IsWindowVisible"), body.index("PtInRect"))
        self.assertIn("GetWindowRect", body)

    def test_fullscreen_geometry_uses_10px_tolerance(self):
        self.assertRegex(self.src, r"TOLERANCE:\s*i32\s*=\s*10")
        self.assertIn("GetForegroundWindow", self.src)
        self.assertIn("GetMonitorInfoW", self.src)

    def test_fullscreen_excludes_own_process(self):
        self.assertIn("GetWindowThreadProcessId", self.src)
        self.assertRegex(self.src, r"pid\s*==\s*std::process::id\(\)")

    def test_passive_show_hides_only_when_unpinned_and_not_reveal(self):
        body = fn_body_at(self.src, 0, "show_pet_passively")
        self.assertIn("pinned || reveal", body)
        self.assertIn("system_is_fullscreen", body)

    def test_reveal_raises_topmost_regardless_of_fullscreen_detection(self):
        """唤起即临时置顶：只在检测到全屏时置顶会漏判，漏判就被全屏应用整块盖住。"""
        body = fn_body_at(self.src, 0, "show_pet_passively")
        self.assertGreaterEqual(
            body.count("raise_above_fullscreen"), 2, "显示前后各压一次 TOPMOST"
        )
        self.assertIn("TEMPORARY_TOPMOST", body)
        self.assertIsNone(
            re.search(r"reveal\s*&&\s*system_is_fullscreen\(\)", body),
            "临时置顶不得再绑定在全屏检测上",
        )

    def test_hide_and_unpin_clear_temporary_topmost(self):
        self.assertIn("TEMPORARY_TOPMOST", fn_body_at(self.src, 0, "hide_pet"))
        unpin = fn_body_at(self.src, 0, "set_pet_pinned")
        self.assertIn("TEMPORARY_TOPMOST", unpin)
        self.assertNotIn("show_pet_passively(hwnd, false, true)", unpin,
                         "取消置顶不能被唤起路径再压回最前")

    def test_coverage_detects_topmost_foreground(self):
        body = fn_body_at(self.src, 0, "foreground_is_topmost")
        self.assertIn("GWL_EXSTYLE", body)
        self.assertIn("WS_EX_TOPMOST", body)

    def test_space_observer_is_noop_on_windows(self):
        self.assertIn("Ok(())", fn_body_at(self.src, 0, "observe_space_changes"))


class HotkeyContract(unittest.TestCase):
    """hotkey.rs windows 平台 mod：LL 钩子只旁听 + 共享状态机复用。"""

    @classmethod
    def setUpClass(cls):
        cls.hotkey = read(SRC / "hotkey.rs")
        cls.mod = cfg_mod_body(cls.hotkey, r'target_os = "windows"', "platform")

    def test_installs_both_low_level_hooks_with_message_pump(self):
        self.assertIn("SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0)", self.mod)
        self.assertIn("SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0)", self.mod)
        self.assertIn("GetMessageW(&mut msg, None, 0, 0)", self.mod)
        self.assertIn("DispatchMessageW(&msg)", self.mod)

    def test_keyboard_proc_is_listen_only(self):
        body = fn_body_at(self.mod, 0, "keyboard_proc")
        self.assertIn("try_send", body)
        self.assertIsNone(re.search(r"(?<![a-z_])send\(", body))
        for io in ("push_recent", "Clipboard", "fs::", "open_"):
            self.assertNotIn(io, body)
        self.assertRegex(body, r"CallNextHookEx\(None, code, wparam, lparam\)\s*\}$")

    def test_mouse_proc_is_listen_only(self):
        body = fn_body_at(self.mod, 0, "mouse_proc")
        self.assertIn("try_send", body)
        self.assertIsNone(re.search(r"(?<![a-z_])send\(", body))
        self.assertRegex(body, r"CallNextHookEx\(None, code, wparam, lparam\)\s*\}$")

    def test_mouse_clicks_use_shared_point_and_minus_one_sentinel(self):
        body = fn_body_at(self.mod, 0, "mouse_proc")
        self.assertIn("PresentationEvent::Click(", body)
        self.assertIn("-1,", body)
        self.assertIn("x: info.pt.x as f64", body)
        self.assertIn("y: info.pt.y as f64", body)

    def test_copy_on_first_keydown_with_autorepeat_filter(self):
        self.assertIn("0x43", self.mod)  # VK_C
        self.assertIn("0x56", self.mod)  # VK_V
        self.assertIn("if !self.c_down", self.mod)
        self.assertIn("Some(KbdAction::Copy)", self.mod)

    def test_paste_fires_on_keyup_only(self):
        self.assertRegex(
            self.mod,
            r"self\.paste_pending = false;\s*\n\s*return Some\(KbdAction::Paste\)",
        )

    def test_shift_and_alt_block_shortcuts(self):
        self.assertEqual(self.mod.count("!self.shift && !self.alt"), 2)

    def test_timing_constants_match_spec(self):
        self.assertRegex(self.mod, r"COPY_DELAY: Duration = Duration::from_millis\(250\)")
        self.assertRegex(self.mod, r"RETRY_DELAY: Duration = Duration::from_secs\(2\)")

    def test_keeps_120ms_paste_delay_in_shared_layer(self):
        # 粘贴延迟常量在共享 PresentationState（被 windows 复用），不在 windows mod 内重复
        self.assertIn("from_millis(120)", self.hotkey)
        self.assertNotIn("from_millis(120)", self.mod)

    def test_paste_worker_uses_shared_state_machine(self):
        body = fn_body_at(self.mod, 0, "paste_worker")
        self.assertIn("PresentationState::default()", body)
        self.assertIn("state.receive(event, Instant::now())", body)
        self.assertIn("crate::dismiss_pet_on_outside_click(&app, window, point)", body)
        self.assertIn("crate::bring_pet_forward(&app)", body)
        self.assertNotIn("deadline = now +", body)

    def test_hook_reinstall_on_failure(self):
        self.assertIn("UnhookWindowsHookEx(kb)", self.mod)
        self.assertIn("UnhookWindowsHookEx(mb)", self.mod)
        self.assertRegex(self.mod, r"std::thread::sleep\(RETRY_DELAY\)")

    def test_health_gate_emits_only_on_change(self):
        body = fn_body_at(self.mod, 0, "set_healthy")
        self.assertIn("swap", body)
        self.assertIn("previous == healthy", body)
        self.assertIn('emit("permission-granted"', body)
        self.assertIn('emit("no-permission"', body)

    def test_windows_has_no_macos_settings_semantics(self):
        self.assertIn("HOOK_HEALTHY.load(Ordering::SeqCst)", fn_body_at(self.mod, 0, "has_accessibility_permission"))
        self.assertIn("监听仍在恢复中", fn_body_at(self.mod, 0, "open_accessibility_settings"))

    def test_key_state_is_thread_local(self):
        self.assertIn("thread_local!", self.mod)
        self.assertIn("KEY_STATE", self.mod)

    def test_other_platform_stub_narrowed_correctly(self):
        # 非 mac/windows 平台仍保留原空桩（提示语不变）
        stub = cfg_mod_body(
            self.hotkey, r'not\(any\(target_os = "macos", target_os = "windows"\)\)', "platform"
        )
        self.assertIn("全局快捷键监听目前仅支持 macOS", stub)


class MainContract(unittest.TestCase):
    """main.rs：windows 变体接线契约（一律 cfg 显式标记）。"""

    @classmethod
    def setUpClass(cls):
        cls.main = read(SRC / "main.rs")

    def test_windows_modules_are_declared_with_cfg(self):
        self.assertIn('#[cfg(target_os = "windows")]\nmod virtual_desktop;', self.main)
        self.assertIn('#[cfg(target_os = "windows")]\nmod windows_window;', self.main)

    def test_windows_subsystem_attr_for_release(self):
        self.assertRegex(
            cls_main_attr := self.main,
            r"#!\[cfg_attr\(not\(debug_assertions\), windows_subsystem = \"windows\"\)\]",
        )

    def test_hwnd_adapter_wraps_tauri_handle(self):
        body = windows_body(self.main, "pet_hwnd")
        self.assertIn("win.hwnd().map(|h| HWND(h.0))", body)

    def test_pin_applies_native_state_before_updating_atomic(self):
        shared_toggle = fn_body_at(self.main, 0, "toggle_pet_pin")
        self.assertLess(
            shared_toggle.index("set_pet_pinned(&win, pin)"),
            shared_toggle.index("PET_PINNED.store(pin, Ordering::SeqCst)"),
        )
        windows_setter = windows_body(self.main, "set_pet_pinned")
        self.assertIn("windows_window::set_pet_pinned(hwnd, pinned)", windows_setter)

    def test_dismiss_outside_click_order(self):
        body = windows_body(self.main, "dismiss_pet_on_outside_click")
        order = [
            body.index("PET_PINNED.load"),
            body.index("contains_click"),
            body.index("hide_pet"),
        ]
        self.assertEqual(order, sorted(order), "置顶返回 -> 命中豁免 -> 隐藏，次序不可乱")

    def test_screen_context_polls_every_750ms(self):
        body = windows_body(self.main, "observe_screen_context")
        self.assertIn("from_millis(750)", body)
        self.assertIn("windows_window::system_is_fullscreen()", body)
        self.assertIn("covered && !previous", body)

    def test_screen_context_reasserts_temporary_topmost(self):
        body = windows_body(self.main, "observe_screen_context")
        self.assertIn("reassert_temporary_topmost", body)

    def test_only_auto_hidden_pet_returns_after_cover(self):
        """被全屏/最大化挡住自动收起的桌宠才恢复；用户点掉的保持隐藏。"""
        observer = windows_body(self.main, "observe_screen_context")
        self.assertIn("PET_HIDDEN_BY_COVER.swap(false", observer)
        dismiss = windows_body(self.main, "dismiss_pet_on_outside_click")
        self.assertIn("PET_HIDDEN_BY_COVER.store(false", dismiss)

    def test_passive_show_and_fullscreen_store_use_windows_native(self):
        show = windows_body(self.main, "show_pet_passively")
        self.assertIn("windows_window::show_pet_passively(hwnd, pinned, reveal)", show)
        self.assertIn("#[cfg(target_os = \"windows\")]", self.main)
        self.assertRegex(
            self.main,
            r'#\[cfg\(target_os = "windows"\)\]\s*\n\s*PET_SCREEN_COVERED\.store\(unsafe \{ windows_window::system_is_fullscreen\(\) \}',
        )

    def test_setup_configures_pet_on_all_desktops_and_spawns_observer(self):
        main_body = fn_body_at(self.main, 0, "main")
        self.assertIn('cfg(target_os = "windows")', main_body)
        self.assertIn("configure_all_spaces(&pet)", main_body)
        self.assertGreaterEqual(
            main_body.count("observe_screen_context(app.handle().clone())"), 2,
            "mac 与 windows 各自 spawn observe",
        )

    def test_panel_show_focus_but_pet_never(self):
        panel_body = fn_body_at(self.main, 0, "show_panel")
        self.assertIn("set_focus", panel_body)
        pet_body = fn_body_at(self.main, 0, "present_pet")
        self.assertNotIn("set_focus", pet_body)

    def test_dismiss_missing_point_is_a_noop(self):
        body = windows_body(self.main, "dismiss_pet_on_outside_click")
        match = re.search(r"let Some\(point\) = clicked_point else \{\s*return;", body)
        self.assertIsNotNone(match, "无坐标（-1 哨兵无点位）不得隐藏桌宠")


class StoreContract(unittest.TestCase):
    """store.rs：限流、平台路径与原子写契约。"""

    @classmethod
    def setUpClass(cls):
        cls.src = read(SRC / "store.rs")

    def test_windows_localappdata_path(self):
        self.assertIn("#[cfg(target_os = \"windows\")]", self.src)
        self.assertIn("dirs::data_local_dir()", self.src)
        self.assertIn('"desktop-pet-clipboard"', self.src)
        self.assertIn('"store.json"', self.src)

    def test_capacity_and_size_limits(self):
        self.assertRegex(self.src, r"PERMANENT_MAX: usize = 100;")
        self.assertRegex(self.src, r"RECENT_MAX: usize = 10;")
        self.assertRegex(self.src, r"MAX_TEXT_BYTES: usize = 1024 \* 1024;")
        self.assertIn('"key 不能超过 1 KB"', self.src)

    def test_atomic_write_renames_only_after_sync(self):
        self.assertIn("create_new(true)", self.src)
        self.assertIn("sync_all()", self.src)
        self.assertIn("std::fs::rename(&temp, &self.path)", self.src)
        self.assertIn("remove_file(&temp)", self.src)
        self.assertIn("Rename only after a complete write", self.src)
        self.assertIn("failed saves never change in-memory data", self.src)

    def test_unpersisted_placeholders(self):
        self.assertIn("value.trim().is_empty()", self.src)
        self.assertIn("value.len() > MAX_TEXT_BYTES", self.src)

    def test_schema_tolerates_old_fields(self):
        self.assertIn("#[serde(default)]", self.src)


class VirtualDesktopContract(unittest.TestCase):
    """virtual_desktop.rs + windows_window.rs：跨桌面固定失败降级。"""

    @classmethod
    def setUpClass(cls):
        cls.win_src = read(SRC / "windows_window.rs")
        cls.vd_src = read(SRC / "virtual_desktop.rs")

    def test_pin_failure_degrades_without_breaking_startup(self):
        body = fn_body_at(self.win_src, 0, "configure_all_spaces")
        self.assertIn("if let Err", body)
        self.assertIn("eprintln!", body)
        self.assertRegex(body, r"Ok\(\(\)\)\s*\}$")

    def test_com_init_errors_are_tolerated(self):
        self.assertIn("let _ = CoInitializeEx", self.vd_src)
        self.assertIn("pin_window", self.vd_src)
        self.assertIn("is_view_pinned", self.vd_src)


class TauriConfContract(unittest.TestCase):
    """tauri.conf.json + tauri.windows.conf.json：窗口/CSP/平台分开打包。"""

    @classmethod
    def setUpClass(cls):
        cls.conf = json.loads(read(REPO / "src-tauri" / "tauri.conf.json"))
        cls.overlay = json.loads(read(REPO / "src-tauri" / "tauri.windows.conf.json"))

    def test_pet_window_config(self):
        pet = next(w for w in self.conf["app"]["windows"] if w["label"] == "pet")
        self.assertFalse(pet["focusable"])
        self.assertFalse(pet["focus"])
        self.assertFalse(pet["decorations"])
        self.assertTrue(pet["transparent"])
        self.assertTrue(pet["skipTaskbar"])
        self.assertFalse(pet["visible"])
        self.assertFalse(pet["alwaysOnTop"])
        self.assertTrue(pet["acceptFirstMouse"])

    def test_panel_window_config(self):
        panel = next(w for w in self.conf["app"]["windows"] if w["label"] == "panel")
        self.assertFalse(panel["visible"])

    def test_global_tauri_bridge_enabled(self):
        self.assertTrue(self.conf["app"]["withGlobalTauri"])

    def test_csp_locks_down(self):
        csp = self.conf["app"]["security"]["csp"]
        for required in ("default-src 'self'", "script-src 'self'", "frame-ancestors 'none'", "object-src 'none'"):
            self.assertIn(required, csp)

    def test_windows_overlay_bundles_nsis_current_user(self):
        self.assertEqual(self.overlay["bundle"]["targets"], ["nsis"])
        self.assertIn("icons/icon.ico", self.overlay["bundle"]["icon"])
        self.assertEqual(self.overlay["bundle"]["windows"]["nsis"]["installMode"], "currentUser")
        # 主配置不携带 windows/nsis 段（Mac 打包产物零变化）
        self.assertNotIn("windows", self.conf["bundle"])


class CapabilitiesContract(unittest.TestCase):
    """capabilities：最小权限集合。"""

    def test_only_default_and_drag_permissions(self):
        cap = json.loads(read(REPO / "src-tauri" / "capabilities" / "default.json"))
        self.assertEqual(set(cap["windows"]), {"pet", "panel"})
        self.assertEqual(
            set(cap["permissions"]),
            {"core:default", "core:window:allow-start-dragging"},
        )


class FrontendWindowsAdjustmentContract(unittest.TestCase):
    """§6.4 前端微调（合并策略）：HTML 保留 Mac 文案，Windows 文案运行时按 UA 选择。"""

    def test_windows_fonts_in_font_stack(self):
        for name in ("pet.html", "panel.html"):
            html = read(FRONTEND / name)
            self.assertIn('"Microsoft YaHei"', html, f"{name} 字体栈缺少微软雅黑")
            self.assertIn('"Segoe UI"', html, f"{name} 字体栈缺少 Segoe UI")

    def test_windows_copy_lives_behind_navigator_guard(self):
        pet = read(FRONTEND / "pet.js")
        panel = read(FRONTEND / "panel.js")
        guard = "typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent)"
        self.assertIn(guard, pet)
        self.assertIn(guard, panel)
        self.assertIn("'复制/粘贴监听已中断，正在自动恢复；点击重试'", pet)
        self.assertIn("'全局复制/粘贴监听已中断，正在自动恢复，点击重试'", panel)

    def test_html_static_copy_stays_macos(self):
        # Mac 契约锚点：静态文案不被 Windows 化
        pet = read(FRONTEND / "pet.html")
        self.assertIn("「辅助功能」，开启复制和粘贴监听", pet)


class CargoManifestContract(unittest.TestCase):
    """Cargo.toml：依赖特性精确集合 + 合并保护。"""

    @classmethod
    def setUpClass(cls):
        cls.manifest = read(REPO / "src-tauri" / "Cargo.toml")

    def test_windows_features_minimal_set(self):
        block = re.search(
            r'\[target\."?cfg\(windows\)"?\.dependencies\.windows\][\s\S]*?\n\n\[',
            self.manifest,
        )
        self.assertIsNotNone(block, "应存在 cfg(windows) 目标依赖")
        features = set(re.findall(r'"(Win32_[A-Za-z_]+)"', block.group(0)))
        self.assertEqual(
            features,
            {"Win32_Foundation", "Win32_UI_WindowsAndMessaging", "Win32_Graphics_Gdi", "Win32_System_Com"},
        )

    def test_tauri_features_cover_tray_and_png(self):
        self.assertIn('"tray-icon"', self.manifest)
        self.assertIn('"image-png"', self.manifest)


if __name__ == "__main__":
    unittest.main()
