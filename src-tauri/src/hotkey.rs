//! Listen-only global shortcuts. The event tap never performs clipboard or disk I/O.

#[cfg(any(target_os = "macos", test))]
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy)]
enum PresentationEvent {
    Paste,
    Click(i64, Option<Point>),
}

#[cfg(any(target_os = "macos", test))]
#[derive(Default)]
struct PresentationState {
    deadline: Option<std::time::Instant>,
}

#[cfg(any(target_os = "macos", test))]
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
                if action == Some(Shortcut::Copy) {
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

    pub fn spawn_listener(app: tauri::AppHandle, state: AppStateSafe) {
        let (copy_tx, copy_rx) = mpsc::sync_channel(1);
        let (paste_tx, paste_rx) = mpsc::sync_channel(64);
        if CONTEXT
            .set(Context {
                copy_tx,
                paste_tx,
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

#[cfg(not(target_os = "macos"))]
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
    fn ignores_plain_keys_repeats_and_extended_shortcuts() {
        assert_eq!(shortcut(8, 0, false), None);
        assert_eq!(shortcut(9, 0x1000, false), None);
        assert_eq!(shortcut(9, 0x100000, true), None);
        assert_eq!(shortcut(7, 0x100000, false), None);
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
