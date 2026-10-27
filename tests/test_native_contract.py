"""Source/configuration guards, not a substitute for macOS desktop integration tests."""

import json
from html.parser import HTMLParser
from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[1]


class CloseButtonParser(HTMLParser):
    def __init__(self):
        super().__init__()
        self.stack = []
        self.close = None
        self.drag_ancestors = []

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if attrs.get("id") == "closeBtn":
            self.close = attrs
            self.drag_ancestors = [ancestor for ancestor in self.stack
                                   if "data-tauri-drag-region" in ancestor
                                   and ancestor["data-tauri-drag-region"] != "false"]
        if tag not in ("meta", "input", "link", "img", "br"):
            self.stack.append(attrs)

    def handle_endtag(self, tag):
        if self.stack:
            self.stack.pop()


def function_body(source, name):
    start = source.index("fn " + name + "(")
    start = source.index("{", start)
    depth = 1
    end = start + 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


class NativeContractTests(unittest.TestCase):
    def test_pet_is_hidden_and_nonfocusable_until_passive_presentation(self):
        config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
        pet = next(window for window in config["app"]["windows"] if window["label"] == "pet")
        for key in ("visible", "focus", "focusable"):
            self.assertIs(pet[key], False)
        self.assertIs(pet["visibleOnAllWorkspaces"], True)

    def test_passive_path_cannot_use_tauri_show_or_request_focus(self):
        main = (ROOT / "src-tauri/src/main.rs").read_text()
        # The first implementation is macOS; the non-macOS fallback is separate.
        presentation = function_body(main, "show_pet_passively")
        bring_forward = function_body(main, "bring_pet_forward")
        present_pet = function_body(main, "present_pet")
        for body in (presentation, bring_forward, present_pet):
            self.assertNotRegex(body, r"\.show\s*\(|\.set_focus\s*\(|\.activate\s*\(")
        self.assertIn("macos_window::show_pet_passively", presentation)
        native = (ROOT / "src-tauri/src/macos_window.rs").read_text()
        selectors = re.findall(r'b"([^"\n]+)\\0"', native)
        self.assertIn("orderFrontRegardless", selectors)
        for forbidden in ("makeKeyAndOrderFront:", "makeKeyWindow", "activateIgnoringOtherApps:"):
            self.assertNotIn(forbidden, selectors)

    def test_pet_pin_is_independent_of_one_time_paste_presentation(self):
        main = (ROOT / "src-tauri/src/main.rs").read_text()
        toggle = function_body(main, "toggle_pet_pin")
        self.assertIn('get_webview_window("pet")', toggle)
        self.assertNotIn('"panel"', toggle)
        self.assertIn("set_pet_pinned(&win, pin)", toggle)
        self.assertLess(toggle.index("set_pet_pinned(&win, pin)"), toggle.index("PET_PINNED.store"))
        setter = function_body(main, "set_pet_pinned")
        self.assertIn("macos_window::set_pet_pinned", setter)
        self.assertNotIn("set_always_on_top", setter)
        bring_forward = function_body(main, "bring_pet_forward")
        self.assertIn("present_pet(&app2, true)", bring_forward)
        self.assertNotIn("PET_PINNED", bring_forward)
        self.assertNotIn("set_pet_pinned", bring_forward)
        self.assertNotIn("set_always_on_top", bring_forward)
        self.assertNotIn("sink_panel", main)
        native = (ROOT / "src-tauri/src/macos_window.rs").read_text()
        self.assertNotIn("setLevel:", function_body(native, "show_pet_passively"))
        config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
        pet = next(window for window in config["app"]["windows"] if window["label"] == "pet")
        initial = re.search(r"static PET_PINNED: AtomicBool = AtomicBool::new\((true|false)\)", main)
        self.assertIsNotNone(initial)
        self.assertIs(pet["alwaysOnTop"], initial.group(1) == "true")
        self.assertIs(pet["alwaysOnTop"], False)

    def test_one_time_reveal_only_changes_order_not_window_level_or_clipboard(self):
        native = (ROOT / "src-tauri/src/macos_window.rs").read_text()
        presentation = function_body(native, "show_pet_passively")
        self.assertIn("if pinned || reveal", presentation)
        self.assertIn('b"orderFrontRegardless\\0"', presentation)
        self.assertIn("pet_collection_behavior(NSAppKitVersionNumber, pinned, reveal)", presentation)
        self.assertNotIn("setLevel:", presentation)
        main = (ROOT / "src-tauri/src/main.rs").read_text()
        bring_forward = function_body(main, "bring_pet_forward")
        self.assertEqual(bring_forward.count("present_pet(&app2, true)"), 1)
        self.assertNotRegex(bring_forward, r"Clipboard|set_text|copy_to_clipboard|show_panel|toggle_panel")
        self.assertNotIn("PET_PINNED.store", function_body(main, "present_pet"))

    def test_unpin_applies_native_level_then_uses_context_aware_passive_display(self):
        native = (ROOT / "src-tauri/src/macos_window.rs").read_text()
        setter = function_body(native, "set_pet_pinned")
        self.assertIn('b"setLevel:\\0"', setter)
        self.assertIn("pet_level(pinned)", setter)
        self.assertIn("show_pet_passively(window, false, true)", setter)
        self.assertLess(setter.index("pet_level(pinned)"), setter.index("show_pet_passively(window, false, true)"))
        main = (ROOT / "src-tauri/src/main.rs").read_text()
        self.assertIn("present_pet(&handle, false)?", function_body(main, "main"))
        self.assertIn("present_pet(&app2, true)", function_body(main, "show_pet"))

    def test_panel_accepts_the_first_click_without_drag_or_blur_reordering(self):
        config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
        panel = next(window for window in config["app"]["windows"] if window["label"] == "panel")
        self.assertIs(panel["acceptFirstMouse"], True)
        self.assertIs(panel["alwaysOnTop"], False)
        main = (ROOT / "src-tauri/src/main.rs").read_text()
        self.assertNotIn("set_always_on_top", function_body(main, "show_panel"))
        self.assertNotIn("WindowEvent::Focused", main)
        parser = CloseButtonParser()
        parser.feed((ROOT / "frontend/panel.html").read_text())
        self.assertIsNotNone(parser.close)
        self.assertEqual(parser.close["data-tauri-drag-region"], "false")
        self.assertEqual(parser.drag_ancestors, [])

    def test_panel_is_initially_hidden_and_has_no_tray_or_automatic_open_path(self):
        config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())
        panel = next(window for window in config["app"]["windows"] if window["label"] == "panel")
        self.assertIs(panel["visible"], False)
        self.assertIs(panel["focus"], False)
        main = (ROOT / "src-tauri/src/main.rs").read_text()
        self.assertEqual(len(re.findall(r"\bshow_panel\(", main)), 2)  # Definition plus the manual toggle.
        self.assertIn("show_panel(app)", function_body(main, "toggle_panel"))
        self.assertNotIn("float_panel", main)
        self.assertNotIn('"open-panel"', main)
        self.assertIn('"show-pet" => show_pet(app)', main)
        self.assertNotIn("show_panel", function_body(main, "bring_pet_forward"))
        self.assertNotIn("show_panel", function_body(main, "present_pet"))
        self.assertNotIn("show_panel", function_body(main, "show_pet"))

    def test_close_panel_is_idempotent_hiding_not_destruction(self):
        main = (ROOT / "src-tauri/src/main.rs").read_text()
        close = function_body(main, "close_panel")
        self.assertIn('get_webview_window("panel")', close)
        self.assertIn(".hide()", close)
        self.assertNotRegex(close, r"\.close\(|\.destroy\(|show_panel|with_store")

    def test_shortcut_callback_only_queues_work_and_returns_original_event(self):
        hotkey = (ROOT / "src-tauri/src/hotkey.rs").read_text()
        callback = function_body(hotkey, "tap_callback")
        self.assertIn("paste_tx.try_send", callback)
        self.assertIn("copy_tx.try_send", callback)
        self.assertNotRegex(callback, r"bring_pet_forward|Clipboard::|\.set_text\(|CGEventPost|std::thread")
        self.assertNotIn("null_mut()", callback)
        self.assertRegex(callback, r"event\s*}$")
        self.assertRegex(hotkey, r"const LISTEN_ONLY:\s*u32\s*=\s*1;")
        self.assertIn("LISTEN_ONLY,", hotkey)

    def test_outside_click_dismissal_only_exempts_the_pet_in_every_screen_mode(self):
        main = (ROOT / "src-tauri/src/main.rs").read_text()
        dismiss = function_body(main, "dismiss_pet_on_outside_click")
        self.assertIn("PET_PINNED.load", dismiss)
        self.assertNotIn("PET_REVEALED", dismiss)  # Stale/unrevealed pets must also dismiss.
        self.assertIn("clicked_window == -1", dismiss)
        self.assertIn('get_webview_window("pet")', dismiss)
        self.assertNotIn('"panel"', dismiss)
        self.assertIn("macos_window::window_number", dismiss)
        self.assertIn("macos_window::hide_pet", dismiss)
        self.assertIn("macos_window::contains_click", dismiss)
        self.assertIn("macos_window::system_is_fullscreen()", dismiss)
        self.assertNotIn("present_pet", dismiss)
        click_path = dismiss[dismiss.index("// Tap"):]
        self.assertNotIn("PET_PINNED.load", click_path)
        self.assertNotIn("system_is_fullscreen", click_path)
        self.assertRegex(dismiss, r"clicked_window == -1\s*&&")
        self.assertLess(dismiss.index("macos_window::system_is_fullscreen()"),
                        dismiss.index("macos_window::hide_pet"))
        self.assertLess(dismiss.index("macos_window::contains_click"),
                        dismiss.index("macos_window::hide_pet"))
        self.assertNotRegex(dismiss, r"\.set_focus\(|\.show\(|set_pet_pinned|PET_PINNED.store|show_panel|\.close\(")
        native = (ROOT / "src-tauri/src/macos_window.rs").read_text()
        self.assertIn('b"windowNumber\\0"', function_body(native, "window_number"))
        # Real removal from WindowServer, not just a different order in the same full-screen Space.
        hide = function_body(native, "hide_pet")
        self.assertIn('b"orderOut:\\0"', hide)
        self.assertIn('b"isVisible\\0"', hide)
        self.assertLess(hide.index('b"orderOut:'), hide.index('b"isVisible'))
        self.assertIn("Err(", hide)
        self.assertNotRegex(hide, r"order_back|orderFront|setLevel")

    def test_click_hit_testing_uses_event_coordinates_and_visible_native_window_frames(self):
        native = (ROOT / "src-tauri/src/macos_window.rs").read_text()
        hit = function_body(native, "contains_click")
        self.assertIn('b"isVisible\\0"', hit)
        self.assertIn('b"screens\\0"', hit)
        self.assertIn("primary_frame.origin.y + primary_frame.size.y - point.y", hit)
        self.assertIn("frame(window).contains(cocoa_point)", hit)
        self.assertNotIn("cursor_position", hit)
        hotkey = (ROOT / "src-tauri/src/hotkey.rs").read_text()
        callback = function_body(hotkey, "tap_callback")
        self.assertIn("CGEventGetLocation(event)", callback)
        self.assertNotIn("contains_click", callback)

    def test_screen_detection_includes_foreground_window_geometry_without_screen_capture(self):
        native = (ROOT / "src-tauri/src/macos_window.rs").read_text()
        context = function_body(native, "system_is_fullscreen")
        self.assertIn("front_window_covers_screen()", context)
        detect = function_body(native, "front_window_covers_screen")
        self.assertIn("CGWindowListCopyWindowInfo(1 | 16, 0)", detect)
        self.assertIn("kCGWindowOwnerPID", detect)
        self.assertIn("kCGWindowLayer", detect)
        self.assertIn("kCGWindowBounds", detect)
        self.assertIn("std::process::id()", detect)
        self.assertIn("covers_work_area(rect, work_area)", detect)
        self.assertNotRegex(native, r"CGWindowListCreateImage|CGDisplayCreateImage|kCGWindowName")

    def test_window_zoom_and_restore_are_observed_without_repeatedly_hiding_paste_reveals(self):
        main = (ROOT / "src-tauri/src/main.rs").read_text()
        monitor = function_body(main, "observe_screen_context")
        self.assertIn("covered && !previous", monitor)
        self.assertIn("!PET_PINNED.load", monitor)
        self.assertIn("CONTEXT_CHECK_PENDING.swap", monitor)
        self.assertIn("run_on_main_thread", monitor)
        self.assertIn("present_pet(&handle, false)", monitor)
        self.assertIn("PET_SCREEN_COVERED.store", function_body(main, "present_pet"))
        self.assertIn("observe_screen_context(app.handle().clone())", function_body(main, "main"))

    def test_build_identity_is_visible_in_tray_menu(self):
        main = (ROOT / "src-tauri/src/main.rs").read_text()
        self.assertIn('"build-info"', main)
        self.assertIn("1027-5", main)

    def test_launching_inside_an_existing_fullscreen_hides_without_waiting_for_space_change(self):
        native = (ROOT / "src-tauri/src/macos_window.rs").read_text()
        presentation = function_body(native, "show_pet_passively")
        self.assertIn("else if system_is_fullscreen()", presentation)
        self.assertIn("hide_pet(window)", presentation)
        fullscreen = function_body(native, "system_is_fullscreen")
        self.assertIn('b"currentSystemPresentationOptions\\0"', fullscreen)
        self.assertNotIn('b"presentationOptions\\0"', fullscreen)

    def test_mouse_listening_is_passive_and_cancels_pending_paste_before_dismissal(self):
        hotkey = (ROOT / "src-tauri/src/hotkey.rs").read_text()
        callback = function_body(hotkey, "tap_callback")
        self.assertIn("Some(CGEventGetLocation(event))", callback)
        self.assertNotIn("dismiss_pet_on_outside_click", callback)
        for event in ("EVENT_LEFT_MOUSE_DOWN", "EVENT_RIGHT_MOUSE_DOWN", "EVENT_OTHER_MOUSE_DOWN"):
            self.assertIn(event, callback)
            self.assertIn("(1 << " + event + ")", hotkey)
        self.assertIn("crate::dismiss_pet_on_outside_click", function_body(hotkey, "paste_worker"))
        self.assertIn("self.deadline = None", function_body(hotkey, "receive"))

    def test_space_changes_clear_temporary_reveal_instead_of_carrying_it_into_fullscreen(self):
        native = (ROOT / "src-tauri/src/macos_window.rs").read_text()
        self.assertIn("NSWorkspaceActiveSpaceDidChangeNotification", function_body(native, "observe_space_changes"))
        notification = function_body(native, "space_did_change")
        self.assertIn("dismiss_for_space_change()", notification)
        self.assertNotRegex(notification, r"orderFront|setLevel|\.show\(|\.set_focus\(")
        hotkey = (ROOT / "src-tauri/src/hotkey.rs").read_text()
        self.assertIn("PresentationEvent::Click(-1, None)", function_body(hotkey, "dismiss_for_space_change"))
        main = (ROOT / "src-tauri/src/main.rs").read_text()
        self.assertIn("macos_window::observe_space_changes()?", function_body(main, "main"))

    def test_agent_launch_policy_is_explicit(self):
        main = (ROOT / "src-tauri/src/main.rs").read_text()
        self.assertIn("set_activation_policy(tauri::ActivationPolicy::Accessory)", main)
        self.assertIn("activate_ignoring_other_apps(false)", main)
        self.assertIn('"quit" => app.exit(0)', main)


if __name__ == "__main__":
    unittest.main()
