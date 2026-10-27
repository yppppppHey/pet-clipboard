//! Main-thread-only AppKit operations. Passive presentation must never request a key window.

use std::ffi::{c_char, c_void};
use crate::hotkey::Point;
use std::sync::atomic::{AtomicBool, Ordering};

static LAST_SCREEN_CONTEXT: AtomicBool = AtomicBool::new(false);

#[link(name = "objc")]
extern "C" {
    fn objc_msgSend();
    fn sel_registerName(name: *const c_char) -> *const c_void;
    fn objc_getClass(name: *const c_char) -> *const c_void;
    fn objc_allocateClassPair(
        superclass: *const c_void,
        name: *const c_char,
        extra: usize,
    ) -> *const c_void;
    fn objc_registerClassPair(class: *const c_void);
    fn class_addMethod(
        class: *const c_void,
        selector: *const c_void,
        implementation: unsafe extern "C" fn(),
        types: *const c_char,
    ) -> bool;
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    static NSAppKitVersionNumber: f64;
    static NSWorkspaceActiveSpaceDidChangeNotification: *const c_void;
}

const APPKIT_MACOS_13: f64 = 2299.0;
const CAN_JOIN_ALL_SPACES: u64 = 1 << 0;
const TRANSIENT: u64 = 1 << 3;
const IGNORES_CYCLE: u64 = 1 << 6;
const FULL_SCREEN_AUXILIARY: u64 = 1 << 8;
const FULL_SCREEN_DISALLOWS_TILING: u64 = 1 << 12;
const CAN_JOIN_ALL_APPLICATIONS: u64 = 1 << 18;

unsafe extern "C" fn space_did_change(
    _observer: *const c_void,
    _selector: *const c_void,
    _notification: *const c_void,
) {
    // The notification only queues work; the normal main-thread dismissal path owns the windows.
    crate::hotkey::dismiss_for_space_change();
}

pub unsafe fn observe_space_changes() -> Result<(), String> {
    let class = objc_allocateClassPair(
        objc_getClass(b"NSObject\0".as_ptr().cast()),
        b"DPCSpaceObserver\0".as_ptr().cast(),
        0,
    );
    if class.is_null() {
        return Err("无法创建桌面切换监听器".into());
    }
    let selector = sel_registerName(b"spaceDidChange:\0".as_ptr().cast());
    let implementation = std::mem::transmute::<
        unsafe extern "C" fn(*const c_void, *const c_void, *const c_void),
        unsafe extern "C" fn(),
    >(space_did_change);
    if !class_addMethod(class, selector, implementation, b"v@:@\0".as_ptr().cast()) {
        return Err("无法注册桌面切换监听器".into());
    }
    objc_registerClassPair(class);
    let send: unsafe extern "C" fn(*const c_void, *const c_void) -> *const c_void =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    // Keep this +1 retained singleton for the process lifetime; selector observers are not retained.
    let observer = send(class, sel_registerName(b"new\0".as_ptr().cast()));
    let workspace = send(
        objc_getClass(b"NSWorkspace\0".as_ptr().cast()),
        sel_registerName(b"sharedWorkspace\0".as_ptr().cast()),
    );
    let center = send(
        workspace,
        sel_registerName(b"notificationCenter\0".as_ptr().cast()),
    );
    if observer.is_null() || center.is_null() {
        return Err("桌面切换通知不可用".into());
    }
    let add: unsafe extern "C" fn(
        *const c_void,
        *const c_void,
        *const c_void,
        *const c_void,
        *const c_void,
        *const c_void,
    ) = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    add(
        center,
        sel_registerName(b"addObserver:selector:name:object:\0".as_ptr().cast()),
        observer,
        selector,
        NSWorkspaceActiveSpaceDidChangeNotification,
        std::ptr::null(),
    );
    Ok(())
}

fn collection_behavior(appkit_version: f64) -> u64 {
    let base = CAN_JOIN_ALL_SPACES
        | TRANSIENT
        | IGNORES_CYCLE
        | FULL_SCREEN_AUXILIARY
        | FULL_SCREEN_DISALLOWS_TILING;
    if appkit_version >= APPKIT_MACOS_13 {
        // AllSpaces alone does not admit an overlay into another app's full-screen space.
        base | CAN_JOIN_ALL_APPLICATIONS
    } else {
        base
    }
}

fn pet_collection_behavior(appkit_version: f64, pinned: bool, reveal: bool) -> u64 {
    if pinned || reveal {
        // Full-screen eligibility is independent of persistent always-on-top level.
        collection_behavior(appkit_version)
    } else {
        // An unpinned pet is an ordinary window, not another app's full-screen overlay.
        CAN_JOIN_ALL_SPACES | IGNORES_CYCLE
    }
}

fn pet_level(pinned: bool) -> isize {
    if pinned {
        3
    } else {
        0
    }
}

unsafe fn set_collection_behavior(window: *const c_void, behavior: u64) {
    let send: unsafe extern "C" fn(*const c_void, *const c_void, u64) =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    send(
        window,
        sel_registerName(b"setCollectionBehavior:\0".as_ptr().cast()),
        behavior,
    );
}

pub unsafe fn configure_all_spaces(window: *const c_void) {
    set_collection_behavior(window, collection_behavior(NSAppKitVersionNumber));
}

fn is_fullscreen_presentation(options: u64) -> bool {
    options & (1 << 10) != 0
}

pub unsafe fn system_is_fullscreen() -> bool {
    let send: unsafe extern "C" fn(*const c_void, *const c_void) -> *const c_void =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    let app = send(
        objc_getClass(b"NSApplication\0".as_ptr().cast()),
        sel_registerName(b"sharedApplication\0".as_ptr().cast()),
    );
    let options: unsafe extern "C" fn(*const c_void, *const c_void) -> u64 =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    let native_fullscreen = is_fullscreen_presentation(options(
        app,
        sel_registerName(b"currentSystemPresentationOptions\0".as_ptr().cast()),
    ));
    // AppKit presentation flags alone miss zoomed/maximized browser windows, and can
    // describe our accessory panel rather than the application underneath it.
    let covered = front_window_covers_screen().unwrap_or_else(|| {
        LAST_SCREEN_CONTEXT.load(Ordering::SeqCst)
    });
    let fullscreen = native_fullscreen || covered;
    LAST_SCREEN_CONTEXT.store(fullscreen, Ordering::SeqCst);
    fullscreen
}

pub unsafe fn show_pet_passively(
    window: *const c_void,
    pinned: bool,
    reveal: bool,
) -> Result<(), String> {
    set_collection_behavior(
        window,
        pet_collection_behavior(NSAppKitVersionNumber, pinned, reveal),
    );
    let send_bool: unsafe extern "C" fn(*const c_void, *const c_void, i8) =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    send_bool(
        window,
        sel_registerName(b"setHidesOnDeactivate:\0".as_ptr().cast()),
        0,
    );
    // Preserve the chosen window level and never request a key window.
    if pinned || reveal {
        let send: unsafe extern "C" fn(*const c_void, *const c_void) =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(
            window,
            sel_registerName(b"orderFrontRegardless\0".as_ptr().cast()),
        );
        Ok(())
    } else if system_is_fullscreen() {
        // Starting while another app is already full-screen produces no Space-change notification.
        hide_pet(window)
    } else {
        order_back(window);
        Ok(())
    }
}

pub unsafe fn hide_pet(window: *const c_void) -> Result<(), String> {
    set_collection_behavior(
        window,
        pet_collection_behavior(NSAppKitVersionNumber, false, false),
    );
    let send: unsafe extern "C" fn(*const c_void, *const c_void, *const c_void) =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    // Ordering behind does not hide an auxiliary window inside a full-screen Space.
    send(
        window,
        sel_registerName(b"orderOut:\0".as_ptr().cast()),
        std::ptr::null(),
    );
    let visible: unsafe extern "C" fn(*const c_void, *const c_void) -> i8 =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    if visible(window, sel_registerName(b"isVisible\0".as_ptr().cast())) != 0 {
        Err("桌宠原生隐藏失败：orderOut 后窗口仍然可见".into())
    } else {
        Ok(())
    }
}

pub unsafe fn set_pet_pinned(window: *const c_void, pinned: bool) -> Result<(), String> {
    let send_level: unsafe extern "C" fn(*const c_void, *const c_void, isize) =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    send_level(
        window,
        sel_registerName(b"setLevel:\0".as_ptr().cast()),
        pet_level(pinned),
    );
    if pinned {
        show_pet_passively(window, true, false)
    } else {
        // The pin control is part of the pet: lowering its level must not dismiss it.
        show_pet_passively(window, false, true)
    }
}

unsafe fn order_back(window: *const c_void) {
    let send: unsafe extern "C" fn(*const c_void, *const c_void, *const c_void) =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    send(
        window,
        sel_registerName(b"orderBack:\0".as_ptr().cast()),
        std::ptr::null(),
    );
}

pub unsafe fn window_number(window: *const c_void) -> i64 {
    let send: unsafe extern "C" fn(*const c_void, *const c_void) -> isize =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    send(window, sel_registerName(b"windowNumber\0".as_ptr().cast())) as i64
}

// CGEvent locations and Cocoa screen frames use logical points, not Retina pixels.
#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    origin: Point,
    size: Point,
}

impl Rect {
    fn contains(self, point: Point) -> bool {
        point.x >= self.origin.x && point.x < self.origin.x + self.size.x
            && point.y >= self.origin.y && point.y < self.origin.y + self.size.y
    }
}

fn covers_work_area(window: Rect, work_area: Rect) -> bool {
    const TOLERANCE: f64 = 10.0;
    window.origin.x.is_finite() && window.origin.y.is_finite()
        && window.size.x.is_finite() && window.size.y.is_finite()
        && work_area.size.x > 0.0 && work_area.size.y > 0.0
        && window.origin.x <= work_area.origin.x + TOLERANCE
        && window.origin.y <= work_area.origin.y + TOLERANCE
        && window.origin.x + window.size.x >= work_area.origin.x + work_area.size.x - TOLERANCE
        && window.origin.y + window.size.y >= work_area.origin.y + work_area.size.y - TOLERANCE
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(options: u32, relative_to: u32) -> *const c_void;
    fn CGRectMakeWithDictionaryRepresentation(dictionary: *const c_void, rect: *mut Rect) -> bool;
    static kCGWindowOwnerPID: *const c_void;
    static kCGWindowLayer: *const c_void;
    static kCGWindowBounds: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(value: *const c_void);
}

struct CopiedWindows(*const c_void);
impl Drop for CopiedWindows {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0); }
    }
}

unsafe fn front_window_covers_screen() -> Option<bool> {
    let send: unsafe extern "C" fn(*const c_void, *const c_void) -> *const c_void =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    let integer: unsafe extern "C" fn(*const c_void, *const c_void) -> isize =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    let at: unsafe extern "C" fn(*const c_void, *const c_void, usize) -> *const c_void =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    let get: unsafe extern "C" fn(*const c_void, *const c_void, *const c_void) -> *const c_void =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    let workspace = send(objc_getClass(b"NSWorkspace\0".as_ptr().cast()),
        sel_registerName(b"sharedWorkspace\0".as_ptr().cast()));
    let front = send(workspace, sel_registerName(b"frontmostApplication\0".as_ptr().cast()));
    if front.is_null() { return None; }
    let pid = integer(front, sel_registerName(b"processIdentifier\0".as_ptr().cast()));
    if pid == std::process::id() as isize {
        // Opening our panel must not reset the full-screen context behind it.
        return None;
    }
    // WindowServer supplies ordered geometry metadata without capturing any screen pixels or titles.
    let windows = CGWindowListCopyWindowInfo(1 | 16, 0); // OnScreenOnly | ExcludeDesktopElements
    if windows.is_null() { return None; }
    let windows = CopiedWindows(windows);
    let count = integer(windows.0, sel_registerName(b"count\0".as_ptr().cast()));
    let object_at = sel_registerName(b"objectAtIndex:\0".as_ptr().cast());
    let object_for = sel_registerName(b"objectForKey:\0".as_ptr().cast());
    let number = sel_registerName(b"integerValue\0".as_ptr().cast());
    let screens = send(objc_getClass(b"NSScreen\0".as_ptr().cast()),
        sel_registerName(b"screens\0".as_ptr().cast()));
    let screen_count = integer(screens, sel_registerName(b"count\0".as_ptr().cast()));
    if screen_count == 0 { return None; }
    let primary = frame(at(screens, object_at, 0));
    for index in 0..count as usize {
        let info = at(windows.0, object_at, index);
        let owner = get(info, object_for, kCGWindowOwnerPID);
        let layer = get(info, object_for, kCGWindowLayer);
        if owner.is_null() || layer.is_null() || integer(owner, number) != pid
            || integer(layer, number) != 0 { continue; }
        let bounds = get(info, object_for, kCGWindowBounds);
        let mut rect = Rect { origin: Point { x: 0.0, y: 0.0 }, size: Point { x: 0.0, y: 0.0 } };
        if bounds.is_null() || !CGRectMakeWithDictionaryRepresentation(bounds, &mut rect) {
            return None;
        }
        // CGWindow bounds are top-left logical points. Cocoa work areas are bottom-left.
        rect.origin.y = primary.origin.y + primary.size.y - rect.origin.y - rect.size.y;
        for screen_index in 0..screen_count as usize {
            let screen = at(screens, object_at, screen_index);
            let work_area = rect_property(screen, b"visibleFrame\0");
            if covers_work_area(rect, work_area) { return Some(true); }
        }
        // Only consider the foreground application's frontmost normal window, not one behind it.
        return Some(false);
    }
    Some(false)
}

unsafe fn frame(object: *const c_void) -> Rect {
    rect_property(object, b"frame\0")
}

unsafe fn rect_property(object: *const c_void, property: &[u8]) -> Rect {
    let selector = sel_registerName(property.as_ptr().cast());
    #[cfg(target_arch = "aarch64")]
    {
        let send: unsafe extern "C" fn(*const c_void, *const c_void) -> Rect =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(object, selector)
    }
    #[cfg(target_arch = "x86_64")]
    {
        extern "C" { fn objc_msgSend_stret(); }
        let mut result = std::mem::MaybeUninit::<Rect>::uninit();
        let send: unsafe extern "C" fn(*mut Rect, *const c_void, *const c_void) =
            std::mem::transmute(objc_msgSend_stret as unsafe extern "C" fn());
        send(result.as_mut_ptr(), object, selector);
        result.assume_init()
    }
}

pub unsafe fn contains_click(window: *const c_void, point: Point) -> bool {
    let visible: unsafe extern "C" fn(*const c_void, *const c_void) -> i8 =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    if visible(window, sel_registerName(b"isVisible\0".as_ptr().cast())) == 0 {
        return false;
    }
    let send: unsafe extern "C" fn(*const c_void, *const c_void) -> *const c_void =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    let screens = send(objc_getClass(b"NSScreen\0".as_ptr().cast()),
        sel_registerName(b"screens\0".as_ptr().cast()));
    let first: unsafe extern "C" fn(*const c_void, *const c_void, usize) -> *const c_void =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    let primary = first(screens, sel_registerName(b"objectAtIndex:\0".as_ptr().cast()), 0);
    let primary_frame = frame(primary);
    // Flip against the primary screen, not the clicked screen; this preserves negative monitor origins.
    let cocoa_point = Point { x: point.x,
        y: primary_frame.origin.y + primary_frame.size.y - point.y };
    frame(window).contains(cocoa_point)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_coverage_includes_maximized_and_native_fullscreen_but_not_normal_windows() {
        let work = Rect { origin: Point { x: 0.0, y: 25.0 }, size: Point { x: 1440.0, y: 820.0 } };
        assert!(covers_work_area(work, work));
        assert!(covers_work_area(Rect { origin: Point { x: 0.0, y: 0.0 },
            size: Point { x: 1440.0, y: 900.0 } }, work));
        assert!(covers_work_area(Rect { origin: Point { x: 8.0, y: 30.0 },
            size: Point { x: 1424.0, y: 810.0 } }, work));
        assert!(!covers_work_area(Rect { origin: Point { x: 100.0, y: 100.0 },
            size: Point { x: 1000.0, y: 700.0 } }, work));
        assert!(!covers_work_area(Rect { origin: work.origin,
            size: Point { x: 720.0, y: 820.0 } }, work));
        assert!(!covers_work_area(Rect { origin: work.origin,
            size: Point { x: f64::NAN, y: 820.0 } }, work));
    }

    #[test]
    fn screen_coverage_uses_each_monitors_work_area_with_negative_origins() {
        let work = Rect { origin: Point { x: -1920.0, y: -200.0 },
            size: Point { x: 1920.0, y: 1050.0 } };
        assert!(covers_work_area(work, work));
        assert!(!covers_work_area(Rect { origin: Point { x: 0.0, y: 0.0 },
            size: work.size }, work));
    }

    #[test]
    fn click_bounds_cover_retina_logical_points_and_negative_monitor_origins() {
        let rect = Rect { origin: Point { x: -1200.0, y: 400.0 },
            size: Point { x: 168.0, y: 140.0 } };
        assert!(rect.contains(Point { x: -1150.0, y: 450.0 }));
        assert!(!rect.contains(Point { x: -1032.0, y: 450.0 }));
        assert!(!rect.contains(Point { x: -1150.0, y: 540.0 }));
        assert!(!rect.contains(Point { x: 0.0, y: 450.0 }));
    }

    #[test]
    fn fullscreen_uses_the_explicit_system_presentation_bit_not_maximization() {
        assert!(!is_fullscreen_presentation(0));
        assert!(!is_fullscreen_presentation((1 << 0) | (1 << 2)));
        assert!(is_fullscreen_presentation(1 << 10));
        assert!(is_fullscreen_presentation((1 << 10) | (1 << 11)));
    }

    #[test]
    fn idle_unpinned_pet_has_normal_level_and_no_cross_application_overlay_role() {
        assert_eq!(pet_level(false), 0);
        assert_eq!(pet_level(true), 3);
        for version in [2113.0, APPKIT_MACOS_13, 3000.0] {
            let normal = pet_collection_behavior(version, false, false);
            assert_ne!(normal & CAN_JOIN_ALL_SPACES, 0);
            assert_eq!(normal & (CAN_JOIN_ALL_APPLICATIONS | FULL_SCREEN_AUXILIARY), 0);
            assert_eq!(
                pet_collection_behavior(version, true, false),
                collection_behavior(version),
            );
        }
    }

    #[test]
    fn one_time_reveal_is_fullscreen_eligible_at_the_normal_window_level() {
        assert_eq!(pet_level(false), 0);
        for version in [APPKIT_MACOS_13, 3000.0] {
            let behavior = pet_collection_behavior(version, false, true);
            assert_ne!(behavior & CAN_JOIN_ALL_APPLICATIONS, 0);
            assert_ne!(behavior & FULL_SCREEN_AUXILIARY, 0);
        }
    }

    #[test]
    fn modern_macos_overlay_can_join_other_applications() {
        for version in [APPKIT_MACOS_13, 2487.0, 3000.0] {
            let behavior = collection_behavior(version);
            assert_ne!(behavior & CAN_JOIN_ALL_APPLICATIONS, 0);
            assert_ne!(behavior & CAN_JOIN_ALL_SPACES, 0);
            assert_ne!(behavior & FULL_SCREEN_AUXILIARY, 0);
            assert_eq!(behavior & ((1 << 16) | (1 << 17)), 0);
            assert_eq!(behavior & ((1 << 7) | (1 << 9)), 0);
        }
    }

    #[test]
    fn older_macos_does_not_receive_unsupported_behavior() {
        let behavior = collection_behavior(2113.0);
        assert_eq!(behavior & CAN_JOIN_ALL_APPLICATIONS, 0);
        assert_ne!(behavior & FULL_SCREEN_AUXILIARY, 0);
        assert_ne!(behavior & IGNORES_CYCLE, 0);
    }
}

