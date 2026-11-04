//! 主线程 Win32 窗口操作。被动显示绝不抢焦点；与 macos_window.rs 行为对等。

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, PtInRect, MONITOR_DEFAULTTONEAREST, MONITORINFO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
    SetWindowPos, ShowWindow, HWND_NOTOPMOST, HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
};

/// 屏幕坐标点（物理像素，整数）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// 前台窗口是否铺满显示器工作区（10px 容差，覆盖 F11 与最大化）。
fn covers_work_area(win: RECT, work: RECT) -> bool {
    const TOLERANCE: i32 = 10;
    (work.right - work.left) > 0
        && (work.bottom - work.top) > 0
        && win.left <= work.left + TOLERANCE
        && win.top <= work.top + TOLERANCE
        && win.right >= work.right - TOLERANCE
        && win.bottom >= work.bottom - TOLERANCE
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
pub unsafe fn show_pet_passively(
    hwnd: HWND,
    pinned: bool,
    reveal: bool,
) -> Result<(), String> {
    if pinned || reveal {
        show_no_activate(hwnd)
    } else if system_is_fullscreen() {
        // 在已全屏的应用中启动不会产生桌面切换通知，直接隐藏。
        hide_pet(hwnd)
    } else {
        show_no_activate(hwnd)
    }
}

/// 原生隐藏并校验可见性。
pub unsafe fn hide_pet(hwnd: HWND) -> Result<(), String> {
    let _ = ShowWindow(hwnd, SW_HIDE);
    if IsWindowVisible(hwnd).as_bool() {
        Err("桌宠原生隐藏失败：SW_HIDE 后窗口仍然可见".into())
    } else {
        Ok(())
    }
}

/// 置顶切换：先应用 TOPMOST/NOTOPMOST，再按 Mac 对等方式被动显示。
pub unsafe fn set_pet_pinned(hwnd: HWND, pinned: bool) -> Result<(), String> {
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
        // 取消置顶是桌宠自身操作，不应把它隐藏。
        show_pet_passively(hwnd, false, true)
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
