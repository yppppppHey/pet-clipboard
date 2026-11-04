#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod hotkey;
#[cfg(target_os = "macos")]
mod macos_window;
mod store;
#[cfg(target_os = "windows")]
mod virtual_desktop;
#[cfg(target_os = "windows")]
mod windows_window;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;

pub type AppStateSafe = Arc<Mutex<store::StoreManager>>;

/// Pet pin state; keep the initial value aligned with the window configuration.
static PET_PINNED: AtomicBool = AtomicBool::new(false);
#[cfg(any(target_os = "macos", target_os = "windows"))]
static PET_SCREEN_COVERED: AtomicBool = AtomicBool::new(false);
#[cfg(any(target_os = "macos", target_os = "windows"))]
static CONTEXT_CHECK_PENDING: AtomicBool = AtomicBool::new(false);

/// Windows：把 Tauri 窗口句柄转成 Win32 HWND。
#[cfg(target_os = "windows")]
fn pet_hwnd(win: &tauri::WebviewWindow) -> Result<HWND, String> {
    win.hwnd().map(|h| HWND(h.0)).map_err(|e| e.to_string())
}

fn configure_all_spaces(win: &tauri::WebviewWindow) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    unsafe {
        macos_window::configure_all_spaces(win.ns_window()?.cast());
    }
    #[cfg(target_os = "windows")]
    {
        // 跨虚拟桌面固定失败时内部降级为单桌面，绝不阻断启动。
        if let Ok(hwnd) = pet_hwnd(win) {
            let _ = unsafe { windows_window::configure_all_spaces(hwnd) };
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = win;
    Ok(())
}

#[cfg(target_os = "macos")]
fn show_pet_passively(win: &tauri::WebviewWindow, pinned: bool, reveal: bool) -> Result<(), String> {
    unsafe {
        macos_window::show_pet_passively(
            win.ns_window().map_err(|e| e.to_string())?.cast(),
            pinned,
            reveal,
        )
    }
}

#[cfg(target_os = "windows")]
fn show_pet_passively(win: &tauri::WebviewWindow, pinned: bool, reveal: bool) -> Result<(), String> {
    let hwnd = pet_hwnd(win)?;
    unsafe { windows_window::show_pet_passively(hwnd, pinned, reveal) }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn show_pet_passively(win: &tauri::WebviewWindow, _pinned: bool, _reveal: bool) -> Result<(), String> {
    win.show().map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn set_pet_pinned(win: &tauri::WebviewWindow, pinned: bool) -> Result<(), String> {
    unsafe {
        // Apply level and ordering synchronously before acknowledging the new state.
        macos_window::set_pet_pinned(win.ns_window().map_err(|e| e.to_string())?.cast(), pinned)
    }
}

#[cfg(target_os = "windows")]
fn set_pet_pinned(win: &tauri::WebviewWindow, pinned: bool) -> Result<(), String> {
    let hwnd = pet_hwnd(win)?;
    unsafe { windows_window::set_pet_pinned(hwnd, pinned) }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn set_pet_pinned(win: &tauri::WebviewWindow, pinned: bool) -> Result<(), String> {
    win.set_always_on_top(pinned).map_err(|e| e.to_string())
}

/// 鼠标当前所在屏（含位置与尺寸，物理像素）
fn active_monitor(app: &tauri::AppHandle) -> Option<tauri::Monitor> {
    app.cursor_position()
        .ok()
        .and_then(|cursor| app.monitor_from_point(cursor.x, cursor.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())
}

/// 把窗口放到指定屏的右下角（保持屏内边距）
fn place_bottom_right(win: &tauri::WebviewWindow, m: &tauri::Monitor, logical_w: f64, logical_h: f64) {
    let scale = m.scale_factor();
    let area = m.work_area();
    let mp = &area.position;
    let ms = &area.size;
    let px = mp.x + ms.width as i32 - (logical_w * scale) as i32 - (16.0 * scale) as i32;
    let py = mp.y + ms.height as i32 - (logical_h * scale) as i32 - (16.0 * scale) as i32;
    let _ = win.set_position(tauri::PhysicalPosition::new(px.max(mp.x), py.max(mp.y)));
}

fn present_pet(app: &tauri::AppHandle, reveal: bool) -> Result<(), String> {
    let win = app.get_webview_window("pet").ok_or("桌宠窗口不可用")?;
    if let Some(monitor) = active_monitor(app) {
        let on_other_screen = win
            .current_monitor()
            .ok()
            .flatten()
            .map(|current| {
                current.position() != monitor.position() || current.size() != monitor.size()
            })
            .unwrap_or(true);
        if on_other_screen {
            place_bottom_right(&win, &monitor, 168.0, 140.0);
        }
    }
    let pinned = PET_PINNED.load(Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    PET_SCREEN_COVERED.store(unsafe { macos_window::system_is_fullscreen() }, Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    PET_SCREEN_COVERED.store(unsafe { windows_window::system_is_fullscreen() }, Ordering::SeqCst);
    show_pet_passively(&win, pinned, reveal)?;
    Ok(())
}

fn show_pet(app: &tauri::AppHandle) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(error) = present_pet(&app2, true) {
            eprintln!("[pet] passive presentation failed: {error}");
        }
    });
}

/// Paste raises the pet once without changing its pin state or requesting input focus.
pub fn bring_pet_forward(app: &tauri::AppHandle) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(error) = present_pet(&app2, true) {
            eprintln!("[pet] passive presentation failed: {error}");
        }
        let _ = tauri::Emitter::emit_to(&app2, "pet", "v-pressed", ());
    });
}

#[cfg(target_os = "macos")]
pub fn dismiss_pet_on_outside_click(
    app: &tauri::AppHandle,
    clicked_window: i64,
    clicked_point: Option<hotkey::Point>,
) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        // A pinned pet stays visible across outside clicks and Space changes.
        if PET_PINNED.load(Ordering::SeqCst) {
            return;
        }
        if clicked_window == -1
            && !unsafe { macos_window::system_is_fullscreen() }
        {
            return;
        }
        // Tap window IDs can be missing or stale. Only the pet itself is exempt.
        if let Some(win) = app2.get_webview_window("pet") {
            if let Ok(ptr) = win.ns_window() {
                if unsafe { macos_window::window_number(ptr.cast()) } == clicked_window
                    || clicked_point.is_some_and(|point| unsafe {
                        macos_window::contains_click(ptr.cast(), point)
                    })
                {
                    return;
                }
            }
        }
        if let Some(win) = app2.get_webview_window("pet") {
            let hidden = win
                .ns_window()
                .map_err(|e| e.to_string())
                .and_then(|ptr| unsafe { macos_window::hide_pet(ptr.cast()) });
            match hidden {
                Ok(()) => {
                    eprintln!("[pet] dismissed: native visibility=false, clicked_window={clicked_window}");
                }
                Err(error) => eprintln!("[pet] dismissal failed: {error}"),
            }
        }
    });
}

/// Windows：LL 钩子拿不到窗口 id（恒传 -1 哨兵）；带坐标即按命中判定，无坐标不隐藏。
#[cfg(target_os = "windows")]
pub fn dismiss_pet_on_outside_click(
    app: &tauri::AppHandle,
    _clicked_window: i64,
    clicked_point: Option<hotkey::Point>,
) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        // A pinned pet stays visible across outside clicks.
        if PET_PINNED.load(Ordering::SeqCst) {
            return;
        }
        let Some(point) = clicked_point else {
            return;
        };
        let Some(win) = app2.get_webview_window("pet") else {
            return;
        };
        let Ok(hwnd) = pet_hwnd(&win) else {
            return;
        };
        let hit = unsafe {
            windows_window::contains_click(
                hwnd,
                windows_window::Point {
                    x: point.x as i32,
                    y: point.y as i32,
                },
            )
        };
        if hit {
            return;
        }
        match unsafe { windows_window::hide_pet(hwnd) } {
            Ok(()) => eprintln!("[pet] dismissed: native visibility=false"),
            Err(error) => eprintln!("[pet] dismissal failed: {error}"),
        }
    });
}

#[cfg(target_os = "macos")]
fn observe_screen_context(app: tauri::AppHandle) {
    // Only entering full-screen changes presentation. Leaving it must not resurrect
    // a pet dismissed by a click; paste or the explicit menu action reveals it again.
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(750));
        if CONTEXT_CHECK_PENDING.swap(true, Ordering::SeqCst) { continue; }
        let handle = app.clone();
        if app.run_on_main_thread(move || {
            CONTEXT_CHECK_PENDING.store(false, Ordering::SeqCst);
            let covered = unsafe { macos_window::system_is_fullscreen() };
            let previous = PET_SCREEN_COVERED.swap(covered, Ordering::SeqCst);
            if covered && !previous && !PET_PINNED.load(Ordering::SeqCst) {
                eprintln!("[pet] screen context changed: covered={covered}");
                if let Err(error) = present_pet(&handle, false) {
                    eprintln!("[pet] context transition failed: {error}");
                }
            }
        }).is_err() {
            CONTEXT_CHECK_PENDING.store(false, Ordering::SeqCst);
            break;
        }
    });
}

/// Windows：与 mac 侧同语义——仅进入全屏时隐藏一次；离开全屏不复活被点击隐藏的桌宠。
#[cfg(target_os = "windows")]
fn observe_screen_context(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(750));
        if CONTEXT_CHECK_PENDING.swap(true, Ordering::SeqCst) {
            continue;
        }
        let handle = app.clone();
        if app
            .run_on_main_thread(move || {
                CONTEXT_CHECK_PENDING.store(false, Ordering::SeqCst);
                let covered = unsafe { windows_window::system_is_fullscreen() };
                let previous = PET_SCREEN_COVERED.swap(covered, Ordering::SeqCst);
                if covered && !previous && !PET_PINNED.load(Ordering::SeqCst) {
                    if let Err(error) = present_pet(&handle, false) {
                        eprintln!("[pet] context transition failed: {error}");
                    }
                }
            })
            .is_err()
        {
            CONTEXT_CHECK_PENDING.store(false, Ordering::SeqCst);
            break;
        }
    });
}

fn show_panel(app: &tauri::AppHandle) -> Result<(), String> {
    let win = app
        .get_webview_window("panel")
        .ok_or("面板窗口不可用")?;
    if !win.is_visible().map_err(|e| e.to_string())? {
        if let Some(monitor) = active_monitor(app) {
            place_bottom_right(&win, &monitor, 380.0, 520.0);
        }
    }
    win.show().map_err(|e| e.to_string())?;
    win.set_focus().map_err(|e| e.to_string())
}

async fn with_store<T: Send + 'static>(
    state: AppStateSafe,
    operation: impl FnOnce(&mut store::StoreManager) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = state.lock().map_err(|_| "剪贴板存储暂不可用".to_string())?;
        operation(&mut store)
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn on_main_thread<T: Send + 'static>(
    app: tauri::AppHandle,
    operation: impl FnOnce(&tauri::AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.try_send(operation(&handle));
    })
    .map_err(|e| e.to_string())?;
    rx.recv().await.ok_or_else(|| "主线程任务已取消".to_string())?
}

#[tauri::command]
async fn add_permanent(
    state: tauri::State<'_, AppStateSafe>,
    key: String,
    value: String,
) -> Result<(), String> {
    with_store(state.inner().clone(), move |store| {
        store.add_permanent(&key, &value)
    })
    .await
}

#[tauri::command]
async fn remove_permanent(state: tauri::State<'_, AppStateSafe>, key: String) -> Result<(), String> {
    with_store(state.inner().clone(), move |store| {
        store.remove_permanent(&key)
    })
    .await
}

#[derive(serde::Serialize)]
struct PermanentRow {
    key: String,
    value: String,
}

#[tauri::command]
async fn search_permanent(
    state: tauri::State<'_, AppStateSafe>,
    query: String,
) -> Result<Vec<PermanentRow>, String> {
    with_store(state.inner().clone(), move |store| {
        Ok(store
            .search_permanent(&query)
            .into_iter()
            .map(|i| PermanentRow {
                key: i.key.clone(),
                value: i.value.clone(),
            })
            .collect())
    })
    .await
}

#[tauri::command]
async fn list_recent(state: tauri::State<'_, AppStateSafe>) -> Result<Vec<String>, String> {
    with_store(state.inner().clone(), |store| {
        Ok(store.recent_list().map(|i| i.value.clone()).collect())
    })
    .await
}

#[tauri::command]
async fn copy_to_clipboard(app: tauri::AppHandle, value: String) -> Result<(), String> {
    on_main_thread(app, move |_| {
        arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(value))
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
async fn toggle_pet_pin(app: tauri::AppHandle) -> Result<bool, String> {
    on_main_thread(app, |app| {
        let win = app.get_webview_window("pet").ok_or("桌宠窗口不可用")?;
        let pin = !PET_PINNED.load(Ordering::SeqCst);
        set_pet_pinned(&win, pin)?;
        PET_PINNED.store(pin, Ordering::SeqCst);
        Ok(pin)
    })
    .await
}

#[tauri::command]
fn is_pet_pinned() -> bool {
    PET_PINNED.load(Ordering::SeqCst)
}

#[tauri::command]
fn permission_status() -> bool {
    hotkey::has_accessibility_permission()
}

#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
    hotkey::open_accessibility_settings()
}

/// 宠物本体按下鼠标 → 系统级拖动窗口
#[tauri::command]
fn start_drag(window: tauri::WebviewWindow) -> Result<(), String> {
    window.start_dragging().map_err(|e| e.to_string())
}

/// 宠物图标点击：打开并聚焦面板，或将其隐藏。
#[tauri::command]
async fn toggle_panel(app: tauri::AppHandle) -> Result<(), String> {
    on_main_thread(app, |app| {
        let win = app
            .get_webview_window("panel")
            .ok_or("面板窗口不可用")?;
        if win.is_visible().map_err(|e| e.to_string())? {
            win.hide().map_err(|e| e.to_string())
        } else {
            show_panel(app)
        }
    })
    .await
}

#[tauri::command]
async fn close_panel(app: tauri::AppHandle) -> Result<(), String> {
    on_main_thread(app, |app| {
        app.get_webview_window("panel")
            .ok_or("面板窗口不可用")?
            .hide()
            .map_err(|e| e.to_string())
    })
    .await
}

fn main() {
    let store = store::StoreManager::load().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.activate_ignoring_other_apps(false);
    builder
        .manage(Arc::new(Mutex::new(store)))
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let state: AppStateSafe = app.state::<AppStateSafe>().inner().clone();
            let handle = app.handle().clone();

            if let Some(panel) = app.get_webview_window("panel") {
                configure_all_spaces(&panel)?;
            }
            if let (Some(pet), Some(monitor)) =
                (app.get_webview_window("pet"), active_monitor(&handle))
            {
                place_bottom_right(&pet, &monitor, 168.0, 140.0);
            }
            present_pet(&handle, false)?;

            // Only the pet's clipboard button may open this initially hidden panel.
            if let Some(panel) = app.get_webview_window("panel") {
                let _ = panel.hide();
            }

            let reveal_pet = tauri::menu::MenuItem::with_id(
                app,
                "show-pet",
                "显示桌宠",
                true,
                None::<&str>,
            )?;
            let quit =
                tauri::menu::MenuItem::with_id(app, "quit", "退出桌宠", true, None::<&str>)?;
            let build_info = tauri::menu::MenuItem::with_id(
                app,
                "build-info",
                "置顶优先修复版 · 1028-1",
                false,
                None::<&str>,
            )?;
            let tray_menu = tauri::menu::Menu::with_items(app, &[&build_info, &reveal_pet, &quit])?;
            let menu_icon = tauri::image::Image::from_bytes(include_bytes!("../menu_icon.png"))?;
            tauri::tray::TrayIconBuilder::with_id("pet-tray")
                .icon(menu_icon)
                .tooltip("宠物剪贴板")
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show-pet" => show_pet(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // 全局按键旁听线程
            hotkey::spawn_listener(handle, state);
            #[cfg(target_os = "macos")]
            unsafe {
                macos_window::observe_space_changes()?;
                observe_screen_context(app.handle().clone());
            }
            #[cfg(target_os = "windows")]
            {
                // 共享调用点只配置了面板；Windows 需把桌宠也固定到所有虚拟桌面。
                if let Some(pet) = app.get_webview_window("pet") {
                    let _ = configure_all_spaces(&pet);
                }
                // 与 mac 平台接口对等：Windows 无 Space 变更事件，此调用为契约 no-op。
                let _ = unsafe { windows_window::observe_space_changes() };
                observe_screen_context(app.handle().clone());
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            add_permanent,
            remove_permanent,
            search_permanent,
            list_recent,
            copy_to_clipboard,
            toggle_pet_pin,
            is_pet_pinned,
            permission_status,
            open_accessibility_settings,
            toggle_panel,
            close_panel,
            start_drag,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
