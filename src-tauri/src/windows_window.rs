//! 主线程 Win32 窗口操作。被动显示绝不抢焦点；与 macos_window.rs 行为对等。

use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, PtInRect, MONITOR_DEFAULTTONEAREST, MONITORINFO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
    SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_NOTOPMOST, HWND_TOPMOST, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_TOPMOST,
};

/// 唤起（粘贴/快捷键）导致的一次性置顶：随时可撤，且随窗口隐藏失效。
/// 固定置顶走 PET_PINNED，与此标记互斥。
static TEMPORARY_TOPMOST: AtomicBool = AtomicBool::new(false);

/// 当前是否处于「唤起导致的临时置顶」。
pub fn temporary_topmost() -> bool {
    TEMPORARY_TOPMOST.load(Ordering::SeqCst)
}

/// 屏幕坐标点（物理像素，整数）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// 前台是否铺满显示器工作区（10px 容差，覆盖 F11 与最大化）。
fn covers_work_area(win: RECT, work: RECT) -> bool {
    const TOLERANCE: i32 = 10;
    (work.right - work.left) > 0
        && (work.bottom - work.top) > 0
        && win.left <= work.left + TOLERANCE
        && win.top <= work.top + TOLERANCE
        && win.right >= work.right - TOLERANCE
        && win.bottom >= work.bottom - TOLERANCE
}

/// 把窗口压到 TOPMOST 带最前面，使其显示在全屏/置顶应用之上。
/// SetWindowPos 连用时会稳定落在顶层，故唤起路径显示前后各调一次。
pub unsafe fn raise_above_fullscreen(hwnd: HWND) -> Result<(), String> {
    SetWindowPos(
        hwnd,
        Some(HWND_TOPMOST),
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    )
    .map_err(|e| format!("全屏置顶失败：{e}"))
}

/// 撤掉临时置顶，恢复普通窗口层级。
pub unsafe fn clear_temporary_topmost(hwnd: HWND) -> Result<(), String> {
    TEMPORARY_TOPMOST.store(false, Ordering::SeqCst);
    SetWindowPos(
        hwnd,
        Some(HWND_NOTOPMOST),
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    )
    .map_err(|e| format!("恢复窗口层级失败：{e}"))
}

/// 前台窗口是否自带 WS_EX_TOPMOST（全屏播放器、部分游戏/悬浮层会这样）。
/// 这类窗口就算没有铺满工作区，普通窗口也永远盖不住它。
pub unsafe fn foreground_is_topmost() -> bool {
    let fg = GetForegroundWindow();
    if fg.0.is_null() {
        return false;
    }
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(fg, Some(&mut pid));
    if pid == std::process::id() {
        return false;
    }
    (GetWindowLongW(fg, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as i32) != 0
}

/// 全屏/置顶应用可能反复把自身拉回最前；观察线程每轮重申一次临时置顶。
pub unsafe fn reassert_temporary_topmost(hwnd: HWND) {
    if temporary_topmost() && IsWindowVisible(hwnd).as_bool() {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// 全屏/铺满检测：不截屏、不读标题，仅比较前台窗口几何。
pub unsafe fn system_is_fullscreen() -> bool {
    let fg = GetForegroundWindow();
    if fg.0.is_null() {
        return false;
    }
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(fg, Some(&mut pid));
    // 打开自己的面板不能改变后台全屏上下文。
    if pid == std::process::id() {
        return false;
    }
    let monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
    if monitor.0.is_null() {
        return false;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(monitor, &mut info).as_bool() {
        return false;
    }
    let mut rect = RECT::default();
    if GetWindowRect(fg, &mut rect).is_err() {
        return false;
    }
    covers_work_area(rect, info.rcWork)
}

unsafe fn show_no_activate(hwnd: HWND) -> Result<(), String> {
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    if !IsWindowVisible(hwnd).as_bool() {
        return Err("桌宠被动显示失败：SW_SHOWNOACTIVATE 后窗口仍不可见".into());
    }
    Ok(())
}

/// 被动显示：不抢焦点；未置顶且非 reveal 时若前台全屏则隐藏。
///
/// reveal（粘贴/快捷键唤起）一律临时置顶，而不是「检测到全屏才置顶」：
/// 全屏检测依赖前台窗口几何与焦点，全屏窗口在另一块屏、置顶型全屏应用、
/// 或检测瞬间焦点漂移都会漏判，漏判的结果就是桌宠被全屏应用整块盖住。
pub unsafe fn show_pet_passively(
    hwnd: HWND,
    pinned: bool,
    reveal: bool,
) -> Result<(), String> {
    if pinned || reveal {
        let temporary = reveal && !pinned;
        if temporary {
            let _ = raise_above_fullscreen(hwnd);
            TEMPORARY_TOPMOST.store(true, Ordering::SeqCst);
        }
        show_no_activate(hwnd)?;
        if temporary {
            // ShowWindow 会重排 Z 序：显示完成后再压一次 TOPMOST 才稳。
            raise_above_fullscreen(hwnd)?;
        }
        Ok(())
    } else if system_is_fullscreen() {
        // 在已全屏的应用中启动不会产生桌面切换通知，直接隐藏。
        TEMPORARY_TOPMOST.store(false, Ordering::SeqCst);
        hide_pet(hwnd)
    } else {
        TEMPORARY_TOPMOST.store(false, Ordering::SeqCst);
        show_no_activate(hwnd)
    }
}

/// 原生隐藏并校验可见性。
pub unsafe fn hide_pet(hwnd: HWND) -> Result<(), String> {
    // 隐藏即结束一次唤起：下次显示按当时上下文重新决定层级。
    TEMPORARY_TOPMOST.store(false, Ordering::SeqCst);
    let _ = ShowWindow(hwnd, SW_HIDE);
    if IsWindowVisible(hwnd).as_bool() {
        Err("桌宠原生隐藏失败：SW_HIDE 后窗口仍然可见".into())
    } else {
        Ok(())
    }
}

/// 置顶切换：先应用 TOPMOST/NOTOPMOST，再按 Mac 对等方式被动显示。
pub unsafe fn set_pet_pinned(hwnd: HWND, pinned: bool) -> Result<(), String> {
    // 固定置顶是「长期」层级，接管后清掉唤起时的临时置顶标记，
    // 否则取消固定时会被临时置顶再次压回最前，表现为取消失效。
    TEMPORARY_TOPMOST.store(false, Ordering::SeqCst);
    let insert_after = if pinned {
        HWND_TOPMOST
    } else {
        HWND_NOTOPMOST
    };
    SetWindowPos(
        hwnd,
        Some(insert_after),
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    )
    .map_err(|e| format!("桌宠置顶设置失败：{e}"))?;
    if pinned {
        show_pet_passively(hwnd, true, false)
    } else {
        // 取消置顶是桌宠自身操作：只恢复层级并保持可见，不因前台全屏而隐藏。
        show_no_activate(hwnd)
    }
}

/// 窗口标识：HWND 数值。
#[allow(dead_code)]
pub unsafe fn window_number(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

/// 点击命中：先校验可见，再用窗口矩形做 PtInRect。
pub unsafe fn contains_click(hwnd: HWND, point: Point) -> bool {
    if !IsWindowVisible(hwnd).as_bool() {
        return false;
    }
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
        return false;
    }
    PtInRect(&rect, POINT { x: point.x, y: point.y }).as_bool()
}

/// 跨所有虚拟桌面显示：经内部 COM 固定窗口；失败时仅记录、不阻断启动。
pub unsafe fn configure_all_spaces(hwnd: HWND) -> Result<(), String> {
    if let Err(error) = crate::virtual_desktop::pin_window(hwnd) {
        eprintln!("[virtual-desktop] 跨桌面固定失败（已降级为单桌面）：{error}");
    }
    Ok(())
}

/// Windows 上跨桌面由 COM 固定完成，无需 Space 变更通知，这里保持接口对等。
pub unsafe fn observe_space_changes() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_includes_maximized_and_native_fullscreen() {
        let work = RECT { left: 0, top: 25, right: 1440, bottom: 845 };
        assert!(covers_work_area(work, work));
        assert!(covers_work_area(
            RECT { left: 0, top: 0, right: 1440, bottom: 900 },
            work
        ));
        assert!(covers_work_area(
            RECT { left: 8, top: 30, right: 1432, bottom: 840 },
            work
        ));
        assert!(!covers_work_area(
            RECT { left: 100, top: 100, right: 1100, bottom: 800 },
            work
        ));
        assert!(!covers_work_area(
            RECT { left: 0, top: 25, right: 720, bottom: 845 },
            work
        ));
    }

    #[test]
    fn coverage_handles_negative_monitor_origins() {
        let work = RECT { left: -1920, top: -200, right: 0, bottom: 850 };
        assert!(covers_work_area(work, work));
        assert!(!covers_work_area(
            RECT { left: 0, top: 0, right: 1920, bottom: 1050 },
            work
        ));
    }

    #[test]
    fn point_equality() {
        assert_eq!(Point { x: 1, y: 2 }, Point { x: 1, y: 2 });
        assert_ne!(Point { x: 1, y: 2 }, Point { x: 2, y: 1 });
    }
}
