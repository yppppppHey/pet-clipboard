//! Listen-only global shortcuts. The event tap never performs clipboard or disk I/O.

#[cfg(any(target_os = "macos", target_os = "windows", test))]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
#[derive(Debug, Clone, Copy)]
enum PresentationEvent {
    Paste,
    Click(i64, Option<Point>),
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
#[derive(Default)]
struct PresentationState {
    deadline: Option<std::time::Instant>,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
impl PresentationState {
    fn receive(
        &mut self,
        event: PresentationEvent,
        now: std::time::Instant,
    ) -> Option<(i64, Option<Point>)> {
        match event {
            PresentationEvent::Paste => {
                self.deadline = Some(now + std::time::Duration::from_millis(120));
                None
            }
            PresentationEvent::Click(window, point) => {
                // A click after paste cancels delayed presentation instead of resurrecting the pet.
                self.deadline = None;
                Some((window, point))
            }
        }
    }

    fn take_due(&mut self, now: std::time::Instant) -> bool {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.deadline = None;
            true
        } else {
            false
        }
    }
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, PartialEq, Eq)]
enum Shortcut {
    Copy,
    Paste,
    Cut,
}

#[cfg(any(target_os = "macos", test))]
fn shortcut(keycode: i64, flags: u64, autorepeat: bool) -> Option<Shortcut> {
    const CONTROL: u64 = 0x40000;
    const COMMAND: u64 = 0x100000;
    const SHIFT_OR_OPTION: u64 = 0x20000 | 0x80000;
    if autorepeat || flags & (CONTROL | COMMAND) == 0 || flags & SHIFT_OR_OPTION != 0 {
        return None;
    }
    match keycode {
        8 => Some(Shortcut::Copy),
        9 => Some(Shortcut::Paste),
        7 => Some(Shortcut::Cut),
        _ => None,
    }
}

#[cfg(any(target_os = "macos", test))]
#[derive(Default)]
struct ShortcutState {
    paste_pending: bool,
}

#[cfg(any(target_os = "macos", test))]
impl ShortcutState {
    fn handle(
        &mut self,
        event_type: u32,
        keycode: i64,
        flags: u64,
        autorepeat: bool,
    ) -> Option<Shortcut> {
        match event_type {
            10 if !autorepeat => {
                let action = shortcut(keycode, flags, false);
                if keycode == 9 {
                    self.paste_pending = action == Some(Shortcut::Paste);
                }
                // Copy 与 Cut 均在 keyDown 触发；Paste 走 keyUp 的 pending 状态机。
                if matches!(action, Some(Shortcut::Copy) | Some(Shortcut::Cut)) {
                    action
                } else {
                    None
                }
            }
            11 if keycode == 9 => {
                let pending = std::mem::take(&mut self.paste_pending);
                pending.then_some(Shortcut::Paste)
            }
            _ => None,
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{Point, PresentationEvent, PresentationState, Shortcut, ShortcutState};
    use crate::AppStateSafe;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicPtr, Ordering};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};
    use tauri::Emitter;

    const COPY_DELAY: Duration = Duration::from_millis(250);
    const RETRY_DELAY: Duration = Duration::from_secs(2);
    const EVENT_KEY_DOWN: u32 = 10;
    const EVENT_KEY_UP: u32 = 11;
    const EVENT_LEFT_MOUSE_DOWN: u32 = 1;
    const EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
    const EVENT_OTHER_MOUSE_DOWN: u32 = 25;
    const FIELD_WINDOW_UNDER_POINTER: i32 = 91;
    const FIELD_WINDOW_HANDLING_CLICK: i32 = 92;
    const FIELD_KEYCODE: i32 = 9;
    const FIELD_AUTOREPEAT: i32 = 8;
    const SESSION_TAP: u32 = 1;
    const HEAD_INSERT: u32 = 0;
    const LISTEN_ONLY: u32 = 1;
    const TAP_DISABLED_TIMEOUT: u32 = 0xfffffffe;
    const TAP_DISABLED_USER_INPUT: u32 = 0xffffffff;

    type TapCallback =
        unsafe extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            mask: u64,
            callback: TapCallback,
            user_info: *mut c_void,
        ) -> *mut c_void;
        fn CGEventTapEnable(tap: *mut c_void, enable: bool);
        fn CGEventGetIntegerValueField(event: *mut c_void, field: i32) -> i64;
        fn CGEventGetFlags(event: *mut c_void) -> u64;
        fn CGEventGetLocation(event: *mut c_void) -> Point;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopCommonModes: *const c_void;
        fn CFMachPortCreateRunLoopSource(
            alloc: *const c_void,
            port: *mut c_void,
            order: isize,
        ) -> *mut c_void;
        fn CFMachPortInvalidate(port: *mut c_void);
        fn CFRunLoopGetCurrent() -> *mut c_void;
        fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
        fn CFRunLoopRemoveSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
        fn CFRunLoopRun();
        fn CFRelease(value: *const c_void);
    }

    struct Context {
        copy_tx: SyncSender<()>,
        paste_tx: SyncSender<PresentationEvent>,
        panel_tx: SyncSender<()>,
        tap: AtomicPtr<c_void>,
    }

    static CONTEXT: OnceLock<Context> = OnceLock::new();

    pub fn dismiss_for_space_change() {
        if let Some(context) = CONTEXT.get() {
            // No real window has this ID; also cancel a pending paste on the old Space.
            let _ = context.paste_tx.try_send(PresentationEvent::Click(-1, None));
        }
    }

    pub fn has_accessibility_permission() -> bool {
        unsafe { AXIsProcessTrustedWithOptions(std::ptr::null()) }
    }

    pub fn open_accessibility_settings() -> Result<(), String> {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("打开辅助功能设置失败：{e}"))
    }

    unsafe extern "C" fn tap_callback(
        _proxy: *mut c_void,
        event_type: u32,
        event: *mut c_void,
        user_info: *mut c_void,
    ) -> *mut c_void {
        let Some(context) = CONTEXT.get() else {
            return event;
        };
        if matches!(event_type, TAP_DISABLED_TIMEOUT | TAP_DISABLED_USER_INPUT) {
            if let Some(shortcuts) = user_info.cast::<ShortcutState>().as_mut() {
                shortcuts.paste_pending = false;
            }
            let tap = context.tap.load(Ordering::Relaxed);
            if !tap.is_null() {
                CGEventTapEnable(tap, true);
            }
            return event;
        }
        if event.is_null() {
            return event;
        }
        if matches!(
            event_type,
            EVENT_LEFT_MOUSE_DOWN | EVENT_RIGHT_MOUSE_DOWN | EVENT_OTHER_MOUSE_DOWN
        ) {
            let handler = CGEventGetIntegerValueField(event, FIELD_WINDOW_HANDLING_CLICK);
            let window = if handler > 0 {
                handler
            } else {
                CGEventGetIntegerValueField(event, FIELD_WINDOW_UNDER_POINTER)
            };
            let _ = context.paste_tx.try_send(PresentationEvent::Click(
                window, Some(CGEventGetLocation(event)),
            ));
            return event;
        }
        if !matches!(event_type, EVENT_KEY_DOWN | EVENT_KEY_UP) {
            return event;
        }
        let Some(shortcuts) = user_info.cast::<ShortcutState>().as_mut() else {
            return event;
        };
        match shortcuts.handle(
            event_type,
            CGEventGetIntegerValueField(event, FIELD_KEYCODE),
            CGEventGetFlags(event),
            CGEventGetIntegerValueField(event, FIELD_AUTOREPEAT) != 0,
        ) {
            Some(Shortcut::Copy) => {
                let _ = context.copy_tx.try_send(());
            }
            Some(Shortcut::Paste) => {
                let _ = context.paste_tx.try_send(PresentationEvent::Paste);
            }
            Some(Shortcut::Cut) => {
                // 剪切同样写入剪贴板：复用复制管线收录内容，同时唤出面板。
                let _ = context.copy_tx.try_send(());
                let _ = context.panel_tx.try_send(());
            }
            None => {}
        }
        event
    }

    fn capture_clipboard(app: &tauri::AppHandle, state: &AppStateSafe) -> Result<bool, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        app.run_on_main_thread(move || {
            let text = arboard::Clipboard::new()
                .and_then(|mut clipboard| clipboard.get_text())
                .map_err(|e| e.to_string());
            let _ = tx.send(text);
        })
        .map_err(|e| e.to_string())?;
        // AppKit access stays on the main thread; locking and persistence stay on this worker.
        let Ok(Ok(text)) = rx.recv_timeout(Duration::from_secs(2)) else {
            return Ok(false);
        };
        state
            .lock()
            .map_err(|_| "剪贴板存储暂不可用".to_string())?
            .push_recent(&text)
    }

    fn copy_worker(app: tauri::AppHandle, state: AppStateSafe, rx: Receiver<()>) {
        while rx.recv().is_ok() {
            loop {
                match rx.recv_timeout(COPY_DELAY) {
                    Ok(()) => continue,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            match capture_clipboard(&app, &state) {
                Ok(true) => {
                    let _ = app.emit("clipboard-added", ());
                }
                Ok(false) => {}
                Err(error) => {
                    eprintln!("[clipboard] {error}");
                    let _ = app.emit("clipboard-error", error);
                }
            }
        }
    }

    fn paste_worker(app: tauri::AppHandle, rx: Receiver<PresentationEvent>) {
        let mut state = PresentationState::default();
        loop {
            let event = match state.deadline {
                Some(deadline) => {
                    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                }
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match event {
                Ok(event) => {
                    if let Some((window, point)) = state.receive(event, Instant::now()) {
                        crate::dismiss_pet_on_outside_click(&app, window, point);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if state.take_due(Instant::now()) {
                        // The original paste reaches its target before any presentation.
                        crate::bring_pet_forward(&app);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Cmd+X 唤面板 worker：容量 1 合并连按，唤醒即带出面板（重复按下仅重置计时）。
    fn panel_worker(app: tauri::AppHandle, rx: Receiver<()>) {
        while rx.recv().is_ok() {
            crate::bring_panel_forward(&app);
        }
    }

    pub fn spawn_listener(app: tauri::AppHandle, state: AppStateSafe) {
        let (copy_tx, copy_rx) = mpsc::sync_channel(1);
        let (paste_tx, paste_rx) = mpsc::sync_channel(64);
        let (panel_tx, panel_rx) = mpsc::sync_channel(1);
        if CONTEXT
            .set(Context {
                copy_tx,
                paste_tx,
                panel_tx,
                tap: AtomicPtr::new(std::ptr::null_mut()),
            })
            .is_err()
        {
            return;
        }
        let worker_app = app.clone();
        std::thread::spawn(move || copy_worker(worker_app, state, copy_rx));
        let worker_app = app.clone();
        std::thread::spawn(move || paste_worker(worker_app, paste_rx));
        let worker_app = app.clone();
        std::thread::spawn(move || panel_worker(worker_app, panel_rx));
        std::thread::spawn(move || unsafe {
            let mut notified = false;
            loop {
                if !has_accessibility_permission() {
                    if !notified {
                        let _ = app.emit("no-permission", ());
                        notified = true;
                    }
                    std::thread::sleep(RETRY_DELAY);
                    continue;
                }
                let mut shortcuts = ShortcutState::default();
                // The mutable shortcut state belongs only to this run-loop thread.
                let tap = CGEventTapCreate(
                    SESSION_TAP,
                    HEAD_INSERT,
                    LISTEN_ONLY,
                    (1 << EVENT_KEY_DOWN)
                        | (1 << EVENT_KEY_UP)
                        | (1 << EVENT_LEFT_MOUSE_DOWN)
                        | (1 << EVENT_RIGHT_MOUSE_DOWN)
                        | (1 << EVENT_OTHER_MOUSE_DOWN),
                    tap_callback,
                    (&mut shortcuts as *mut ShortcutState).cast(),
                );
                if tap.is_null() {
                    if !notified {
                        let _ = app.emit("no-permission", ());
                        notified = true;
                    }
                    std::thread::sleep(RETRY_DELAY);
                    continue;
                }
                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                if source.is_null() {
                    CFMachPortInvalidate(tap);
                    CFRelease(tap);
                    std::thread::sleep(RETRY_DELAY);
                    continue;
                }
                let Some(context) = CONTEXT.get() else {
                    return;
                };
                context.tap.store(tap, Ordering::Relaxed);
                let run_loop = CFRunLoopGetCurrent();
                CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap, true);
                let _ = app.emit("permission-granted", ());
                CFRunLoopRun();
                context.tap.store(std::ptr::null_mut(), Ordering::Relaxed);
                CFRunLoopRemoveSource(run_loop, source, kCFRunLoopCommonModes);
                CFMachPortInvalidate(tap);
                CFRelease(source);
                CFRelease(tap);
                notified = false;
                std::thread::sleep(RETRY_DELAY);
            }
        });
    }
}

/// Windows 平台事件源：WH_KEYBOARD_LL / WH_MOUSE_LL 低级钩子只旁听（回调只入队、
/// 立即 CallNextHookEx 放行）；时序状态机复用共享层 PresentationState/PresentationEvent。
#[cfg(target_os = "windows")]
mod platform {
    use super::{Point, PresentationEvent, PresentationState};
    use crate::AppStateSafe;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};
    use tauri::Emitter;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
        UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
        WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN,
        WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    const COPY_DELAY: Duration = Duration::from_millis(250);
    const RETRY_DELAY: Duration = Duration::from_secs(2);

    /// 钩子健康状态：安装失败或消息泵异常退出时转 false（前端据此展示恢复提示）。
    static HOOK_HEALTHY: AtomicBool = AtomicBool::new(true);

    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    enum KbdAction {
        Copy,
        Paste,
        Cut,
    }

    /// VK 状态机：修饰键跟踪 + 长按抑制；复制/剪切首次 KEYDOWN、粘贴 KEYUP（对齐共享层时序语义）。
    #[derive(Default)]
    struct Keys {
        ctrl: bool,
        shift: bool,
        alt: bool,
        c_down: bool,
        v_down: bool,
        x_down: bool,
        paste_pending: bool,
    }

    impl Keys {
        fn event(&mut self, vk: u32, down: bool) -> Option<KbdAction> {
            match vk {
                0x11 | 0xA2 | 0xA3 => {
                    self.ctrl = down;
                    return None;
                }
                0x10 | 0xA0 | 0xA1 => {
                    self.shift = down;
                    return None;
                }
                0x12 | 0xA4 | 0xA5 => {
                    self.alt = down;
                    return None;
                }
                _ => {}
            }
            match vk {
                0x43 => {
                    // VK_C
                    if down {
                        if !self.c_down {
                            self.c_down = true;
                            if self.ctrl && !self.shift && !self.alt {
                                return Some(KbdAction::Copy);
                            }
                        }
                    } else {
                        self.c_down = false;
                    }
                }
                0x56 => {
                    // VK_V
                    if down {
                        if !self.v_down {
                            self.v_down = true;
                            if self.ctrl && !self.shift && !self.alt {
                                self.paste_pending = true;
                            }
                        }
                    } else {
                        self.v_down = false;
                        if self.paste_pending {
                            self.paste_pending = false;
                            return Some(KbdAction::Paste);
                        }
                    }
                }
                0x58 => {
                    // VK_X（Ctrl+X）：首次按下触发，连按由 x_down 抑制；同 Copy 语义。
                    if down {
                        if !self.x_down {
                            self.x_down = true;
                            if self.ctrl && !self.shift && !self.alt {
                                return Some(KbdAction::Cut);
                            }
                        }
                    } else {
                        self.x_down = false;
                    }
                }
                _ => {}
            }
            None
        }
    }

    thread_local! {
        /// LL 钩子回调只在安装线程的消息泵上分发；状态机绑定该线程以抑制长按重复。
        static KEY_STATE: std::cell::RefCell<Keys> = std::cell::RefCell::new(Keys::default());
    }

    struct Context {
        copy_tx: SyncSender<()>,
        paste_tx: SyncSender<PresentationEvent>,
        panel_tx: SyncSender<()>,
    }

    static CONTEXT: OnceLock<Context> = OnceLock::new();

    unsafe extern "system" fn keyboard_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code >= 0 {
            let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let up = matches!(wparam.0 as u32, WM_KEYUP | WM_SYSKEYUP);
            if down || up {
                let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                let action =
                    KEY_STATE.with(|cell| cell.borrow_mut().event(info.vkCode as u32, down));
                if let Some(context) = CONTEXT.get() {
                    match action {
                        Some(KbdAction::Copy) => {
                            let _ = context.copy_tx.try_send(());
                        }
                        Some(KbdAction::Paste) => {
                            let _ = context.paste_tx.try_send(PresentationEvent::Paste);
                        }
                        Some(KbdAction::Cut) => {
                            // 剪切同样写入剪贴板：复用复制管线收录内容，同时唤出面板。
                            let _ = context.copy_tx.try_send(());
                            let _ = context.panel_tx.try_send(());
                        }
                        None => {}
                    }
                }
            }
        }
        // 只旁听：始终放行，不吞键。
        CallNextHookEx(None, code, wparam, lparam)
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let id = wparam.0 as u32;
            if matches!(id, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN) {
                let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                if let Some(context) = CONTEXT.get() {
                    // LL 钩子拿不到窗口 id，用 -1 哨兵；屏幕坐标走共享 Point。
                    let _ = context.paste_tx.try_send(PresentationEvent::Click(
                        -1,
                        Some(Point {
                            x: info.pt.x as f64,
                            y: info.pt.y as f64,
                        }),
                    ));
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    fn capture_clipboard(app: &tauri::AppHandle, state: &AppStateSafe) -> Result<bool, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        app.run_on_main_thread(move || {
            let text = arboard::Clipboard::new()
                .and_then(|mut clipboard| clipboard.get_text())
                .map_err(|e| e.to_string());
            let _ = tx.send(text);
        })
        .map_err(|e| e.to_string())?;
        // 剪贴板读取保持在主线程；锁定与持久化留在此 worker。
        let Ok(Ok(text)) = rx.recv_timeout(Duration::from_secs(2)) else {
            return Ok(false);
        };
        state
            .lock()
            .map_err(|_| "剪贴板存储暂不可用".to_string())?
            .push_recent(&text)
    }

    fn copy_worker(app: tauri::AppHandle, state: AppStateSafe, rx: Receiver<()>) {
        while rx.recv().is_ok() {
            loop {
                match rx.recv_timeout(COPY_DELAY) {
                    Ok(()) => continue,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            match capture_clipboard(&app, &state) {
                Ok(true) => {
                    let _ = app.emit("clipboard-added", ());
                }
                Ok(false) => {}
                Err(error) => {
                    eprintln!("[clipboard] {error}");
                    let _ = app.emit("clipboard-error", error);
                }
            }
        }
    }

    fn paste_worker(app: tauri::AppHandle, rx: Receiver<PresentationEvent>) {
        let mut state = PresentationState::default();
        loop {
            let event = match state.deadline {
                Some(deadline) => {
                    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                }
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match event {
                Ok(event) => {
                    if let Some((window, point)) = state.receive(event, Instant::now()) {
                        crate::dismiss_pet_on_outside_click(&app, window, point);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if state.take_due(Instant::now()) {
                        // 原始粘贴先到达目标，然后桌宠一次性呈现。
                        crate::bring_pet_forward(&app);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn set_healthy(healthy: bool, app: &tauri::AppHandle) {
        let previous = HOOK_HEALTHY.swap(healthy, Ordering::SeqCst);
        if previous == healthy {
            return;
        }
        if healthy {
            let _ = app.emit("permission-granted", ());
        } else {
            // 失败驱动：仅在真正失败时才提示。
            let _ = app.emit("no-permission", ());
        }
    }

    /// Windows 无「辅助功能」授权：以钩子健康状态代替权限语义。
    pub fn has_accessibility_permission() -> bool {
        HOOK_HEALTHY.load(Ordering::SeqCst)
    }

    /// Windows 无授权设置页；真正的恢复由监听线程在消息泵退出后自动重装完成，
    /// 这里仅把健康状态如实反馈给前端，避免用户误以为打开了某个设置页。
    pub fn open_accessibility_settings() -> Result<(), String> {
        if HOOK_HEALTHY.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err("监听仍在恢复中，请稍后再试".into())
        }
    }

    /// Ctrl+X 唤面板 worker：容量 1 合并连按，唤醒即带出面板（重复按下仅重置计时）。
    fn panel_worker(app: tauri::AppHandle, rx: Receiver<()>) {
        while rx.recv().is_ok() {
            crate::bring_panel_forward(&app);
        }
    }

    pub fn spawn_listener(app: tauri::AppHandle, state: AppStateSafe) {
        let (copy_tx, copy_rx) = mpsc::sync_channel(1);
        let (paste_tx, paste_rx) = mpsc::sync_channel(64);
        let (panel_tx, panel_rx) = mpsc::sync_channel(1);
        if CONTEXT
            .set(Context {
                copy_tx,
                paste_tx,
                panel_tx,
            })
            .is_err()
        {
            return;
        }
        let worker_app = app.clone();
        std::thread::spawn(move || copy_worker(worker_app, state, copy_rx));
        let worker_app = app.clone();
        std::thread::spawn(move || paste_worker(worker_app, paste_rx));
        let worker_app = app.clone();
        std::thread::spawn(move || panel_worker(worker_app, panel_rx));

        std::thread::spawn(move || loop {
            let keyboard_hook: windows::core::Result<HHOOK> =
                unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0) };
            let mouse_hook: windows::core::Result<HHOOK> =
                unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) };
            let (kb, mb) = match (keyboard_hook, mouse_hook) {
                (Ok(kb), Ok(mb)) => (kb, mb),
                (Err(e), _) | (_, Err(e)) => {
                    eprintln!("[hotkey] 钩子安装失败：{e}");
                    set_healthy(false, &app);
                    std::thread::sleep(RETRY_DELAY);
                    continue;
                }
            };
            set_healthy(true, &app);
            let mut msg = MSG::default();
            // 消息泵：低级钩子回调在此线程分发；GetMessageW 阻塞即健康。
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.into() {
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            // 消息泵异常退出：卸载钩子并按节拍重装。
            set_healthy(false, &app);
            unsafe {
                let _ = UnhookWindowsHookEx(kb);
                let _ = UnhookWindowsHookEx(mb);
            }
            std::thread::sleep(RETRY_DELAY);
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn copy_fires_once_on_first_keydown_and_repeats_ignored() {
            let mut keys = Keys::default();
            keys.event(0xA2, true); // left ctrl
            assert_eq!(keys.event(0x43, true), Some(KbdAction::Copy));
            assert_eq!(keys.event(0x43, true), None); // autorepeat
            assert_eq!(keys.event(0x43, false), None);
        }

        #[test]
        fn paste_fires_on_keyup_only() {
            let mut keys = Keys::default();
            keys.event(0x11, true); // ctrl
            assert_eq!(keys.event(0x56, true), None);
            assert_eq!(keys.event(0x56, false), Some(KbdAction::Paste));
            assert_eq!(keys.event(0x56, false), None);
        }

        #[test]
        fn modifiers_block_shortcuts() {
            let mut keys = Keys::default();
            keys.event(0x11, true);
            keys.event(0x10, true); // shift
            assert_eq!(keys.event(0x43, true), None);
            let mut other = Keys::default();
            other.event(0x11, true);
            other.event(0x12, true); // alt
            assert_eq!(other.event(0x56, true), None);
            assert_eq!(other.event(0x56, false), None);
        }

        #[test]
        fn plain_keys_do_nothing() {
            let mut keys = Keys::default();
            assert_eq!(keys.event(0x43, true), None);
            assert_eq!(keys.event(0x56, false), None);
        }

        #[test]
        fn paste_release_still_recognized_after_modifier_release() {
            let mut keys = Keys::default();
            keys.event(0x11, true);
            keys.event(0x56, true);
            keys.event(0x11, false);
            assert_eq!(keys.event(0x56, false), Some(KbdAction::Paste));
        }

        #[test]
        fn cut_fires_once_on_first_keydown() {
            let mut keys = Keys::default();
            keys.event(0xA2, true); // left ctrl
            assert_eq!(keys.event(0x58, true), Some(KbdAction::Cut));
            assert_eq!(keys.event(0x58, true), None); // autorepeat 抑制
            assert_eq!(keys.event(0x58, false), None);
        }

        #[test]
        fn modifiers_block_cut() {
            let mut keys = Keys::default();
            keys.event(0x11, true);
            keys.event(0x10, true); // shift
            assert_eq!(keys.event(0x58, true), None);
            let mut other = Keys::default();
            other.event(0x11, true);
            other.event(0x12, true); // alt
            assert_eq!(other.event(0x58, true), None);
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    pub fn has_accessibility_permission() -> bool {
        false
    }
    pub fn open_accessibility_settings() -> Result<(), String> {
        Err("全局快捷键监听目前仅支持 macOS".into())
    }
    pub fn spawn_listener(app: tauri::AppHandle, _state: crate::AppStateSafe) {
        use tauri::Emitter;
        let _ = app.emit("no-permission", ());
    }
}

pub use platform::{has_accessibility_permission, open_accessibility_settings, spawn_listener};
#[cfg(target_os = "macos")]
pub use platform::dismiss_for_space_change;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_reveals_once_after_the_target_receives_the_shortcut() {
        let mut state = PresentationState::default();
        let now = std::time::Instant::now();
        assert_eq!(state.receive(PresentationEvent::Paste, now), None);
        assert!(!state.take_due(now + std::time::Duration::from_millis(119)));
        assert!(state.take_due(now + std::time::Duration::from_millis(120)));
        assert!(!state.take_due(now + std::time::Duration::from_millis(240)));
    }

    #[test]
    fn click_during_paste_delay_cancels_late_reappearance() {
        let mut state = PresentationState::default();
        let now = std::time::Instant::now();
        state.receive(PresentationEvent::Paste, now);
        assert_eq!(state.receive(PresentationEvent::Click(42, None), now), Some((42, None)));
        assert!(!state.take_due(now + std::time::Duration::from_secs(1)));
    }

    #[test]
    fn click_keeps_event_time_position_even_when_the_window_id_is_missing() {
        let mut state = PresentationState::default();
        let now = std::time::Instant::now();
        let point = Point { x: -1100.0, y: 450.0 };
        state.receive(PresentationEvent::Paste, now);
        assert_eq!(state.receive(PresentationEvent::Click(0, Some(point)), now),
            Some((0, Some(point))));
        assert!(!state.take_due(now + std::time::Duration::from_secs(1)));
    }

    #[test]
    fn space_change_cancels_pending_paste_without_inventing_a_mouse_position() {
        let mut state = PresentationState::default();
        let now = std::time::Instant::now();
        state.receive(PresentationEvent::Paste, now);
        assert_eq!(state.receive(PresentationEvent::Click(-1, None), now), Some((-1, None)));
        assert!(!state.take_due(now + std::time::Duration::from_secs(1)));
    }

    #[test]
    fn paste_after_dismissal_can_reveal_the_pet_again() {
        let mut state = PresentationState::default();
        let now = std::time::Instant::now();
        for index in 0..10 {
            let at = now + std::time::Duration::from_secs(index);
            state.receive(PresentationEvent::Paste, at);
            assert!(state.take_due(at + std::time::Duration::from_millis(120)));
            assert_eq!(state.receive(PresentationEvent::Click(9, None), at), Some((9, None)));
        }
    }

    #[test]
    fn quick_repeated_pastes_debounce_without_stale_deadlines() {
        let mut state = PresentationState::default();
        let now = std::time::Instant::now();
        state.receive(PresentationEvent::Paste, now);
        state.receive(
            PresentationEvent::Paste,
            now + std::time::Duration::from_millis(100),
        );
        assert!(!state.take_due(now + std::time::Duration::from_millis(120)));
        assert!(state.take_due(now + std::time::Duration::from_millis(220)));
    }

    #[test]
    fn recognizes_copy_and_paste_with_either_modifier() {
        for flags in [0x40000, 0x100000, 0x140000] {
            assert_eq!(shortcut(8, flags, false), Some(Shortcut::Copy));
            assert_eq!(shortcut(9, flags, false), Some(Shortcut::Paste));
        }
    }

    #[test]
    fn cut_fires_on_key_down_only() {
        let mut state = ShortcutState::default();
        assert_eq!(state.handle(10, 7, 0x100000, false), Some(Shortcut::Cut));
        assert_eq!(state.handle(10, 7, 0x100000, true), None); // autorepeat 抑制
        assert_eq!(state.handle(11, 7, 0x100000, false), None); // keyUp 不触发
    }

    #[test]
    fn cut_accepts_ctrl_or_cmd_but_not_modifiers() {
        for flags in [0x40000, 0x100000, 0x140000] {
            assert_eq!(shortcut(7, flags, false), Some(Shortcut::Cut));
        }
        assert_eq!(shortcut(7, 0x100000 | 0x20000, false), None);
        assert_eq!(shortcut(7, 0x100000 | 0x80000, false), None);
    }

    #[test]
    fn ignores_plain_keys_repeats_and_extended_shortcuts() {
        assert_eq!(shortcut(8, 0, false), None);
        assert_eq!(shortcut(9, 0x1000, false), None);
        assert_eq!(shortcut(9, 0x100000, true), None);
        // 7 号键是 X（Cut 有专属用例）；用 6 号键代表确实不监听的键。
        assert_eq!(shortcut(6, 0x100000, false), None);
        assert_eq!(shortcut(8, 0x100000 | 0x20000, false), None);
        assert_eq!(shortcut(9, 0x100000 | 0x80000, false), None);
    }

    #[test]
    fn paste_never_requests_presentation_on_key_down() {
        let mut state = ShortcutState::default();
        assert_eq!(state.handle(10, 9, 0x100000, false), None);
        assert_eq!(state.handle(11, 9, 0x100000, false), Some(Shortcut::Paste));
        assert_eq!(state.handle(11, 9, 0x100000, false), None);
    }

    #[test]
    fn repeated_pastes_are_independent_even_if_command_stays_down() {
        let mut state = ShortcutState::default();
        for _ in 0..10 {
            assert_eq!(state.handle(10, 9, 0x100000, false), None);
            assert_eq!(state.handle(11, 9, 0x100000, false), Some(Shortcut::Paste));
        }
    }

    #[test]
    fn paste_release_is_recognized_after_modifier_is_released() {
        let mut state = ShortcutState::default();
        assert_eq!(state.handle(10, 9, 0x40000, false), None);
        assert_eq!(state.handle(11, 9, 0, false), Some(Shortcut::Paste));
    }

    #[test]
    fn autorepeat_does_not_schedule_more_presentations() {
        let mut state = ShortcutState::default();
        state.handle(10, 9, 0x100000, false);
        for _ in 0..10 {
            assert_eq!(state.handle(10, 9, 0x100000, true), None);
        }
        assert_eq!(state.handle(11, 9, 0, false), Some(Shortcut::Paste));
        assert_eq!(state.handle(11, 9, 0, false), None);
    }

    #[test]
    fn plain_v_and_extended_pastes_do_not_schedule_a_pet() {
        let mut state = ShortcutState::default();
        for flags in [0, 0x100000 | 0x20000, 0x100000 | 0x80000] {
            assert_eq!(state.handle(10, 9, flags, false), None);
            assert_eq!(state.handle(11, 9, flags, false), None);
        }
    }

    #[test]
    fn copy_still_schedules_capture_on_key_down_only() {
        let mut state = ShortcutState::default();
        assert_eq!(state.handle(10, 8, 0x100000, false), Some(Shortcut::Copy));
        assert_eq!(state.handle(10, 8, 0x100000, true), None);
        assert_eq!(state.handle(11, 8, 0x100000, false), None);
    }
}
