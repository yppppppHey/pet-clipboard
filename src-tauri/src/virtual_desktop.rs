//! 内部（非官方）虚拟桌面 COM：把窗口固定到所有虚拟桌面。
//!
//! 适配 windows crate 0.61：HRESULT / GUID 在 `windows::core`。
//!
//! 实现要点（实测验证）：
//! 1. ImmersiveShell 的 CoCreateInstance **必须直接请求 IServiceProvider**
//!    （IID 6d5140c1...）。不要先取 IUnknown 再手动 QueryInterface——在该对象上
//!    手动 QI 会在 combase.dll 内访问违规（0xc0000005）。因此这里用原生
//!    #[link] 声明 ole32 的 CoCreateInstance，riid 直接传 IServiceProvider。
//! 2. IVirtualDesktopPinnedApps::IsViewPinned 是 out 参数签名
//!    `HRESULT(IApplicationView*, BOOL*)`，不能当返回 BOOL 调用，否则会向
//!    垃圾地址写入，造成堆损坏与延迟崩溃（ntdll 0xc0000005）。

use std::ffi::c_void;
use windows::core::{GUID, HRESULT};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};

const CLSID_IMMERSIVE_SHELL: u128 = 0xc2f03a33_21f5_47fa_b4bb_156362a2f239;
const IID_ISERVICE_PROVIDER: u128 = 0x6d5140c1_7436_11ce_8034_00aa006009fa;
const IID_APP_VIEW_COLLECTION: u128 = 0x1841c6d7_4f9d_42c0_af41_8747538f10e5;
const CLSID_PINNED_APPS: u128 = 0xb5a399e7_1c87_46b8_88e9_fc5747b171bd;
const IID_PINNED_APPS: u128 = 0x4ce81583_1e4c_4632_a621_07a53543148f;

#[repr(C)]
struct IServiceProviderVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    query_service: unsafe extern "system" fn(
        *mut c_void,
        *const GUID,
        *const GUID,
        *mut *mut c_void,
    ) -> HRESULT,
}

#[repr(C)]
struct IServiceProvider {
    vtbl: *const IServiceProviderVtbl,
}

#[repr(C)]
struct IAppViewCollectionVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_views: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    get_views_by_zorder: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    get_views_by_app_id: unsafe extern "system" fn(
        *mut c_void,
        *const u16,
        *mut *mut c_void,
    ) -> HRESULT,
    get_view_for_hwnd: unsafe extern "system" fn(*mut c_void, HWND, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IAppViewCollection {
    vtbl: *const IAppViewCollectionVtbl,
}

#[repr(C)]
struct IPinnedAppsVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    is_app_id_pinned: unsafe extern "system" fn(*mut c_void, *const u16, *mut i32) -> HRESULT,
    pin_app_id: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    unpin_app_id: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    is_view_pinned: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i32) -> HRESULT,
    pin_view: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    unpin_view: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IPinnedApps {
    vtbl: *const IPinnedAppsVtbl,
}

#[link(name = "ole32")]
unsafe extern "system" {
    /// 底层 CoCreateInstance：riid 由调用方指定（这里直接请求 IServiceProvider）。
    fn CoCreateInstance(
        rclsid: *const GUID,
        punkouter: *mut c_void,
        dwclscontext: u32,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT;
}

/// 把指定窗口固定到所有虚拟桌面；已固定则跳过。
pub unsafe fn pin_window(hwnd: HWND) -> Result<(), String> {
    // 已用其他模式初始化时返回 RPC_E_CHANGED_MODE，忽略即可。
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

    // 1. 直接 CoCreateInstance(ImmersiveShell, IID_IServiceProvider)。
    let shell_clsid = GUID::from_u128(CLSID_IMMERSIVE_SHELL);
    let sp_iid = GUID::from_u128(IID_ISERVICE_PROVIDER);
    let mut sp: *mut c_void = std::ptr::null_mut();
    let hr = unsafe {
        CoCreateInstance(
            &shell_clsid,
            std::ptr::null_mut(),
            CLSCTX_ALL.0,
            &sp_iid,
            &mut sp,
        )
    };
    if hr.is_err() || sp.is_null() {
        return Err(format!("CoCreateInstance(ImmersiveShell, IServiceProvider) 失败：{hr:?}"));
    }
    let sp_vtbl: &IServiceProviderVtbl = &*(*(sp as *const IServiceProvider)).vtbl;

    // 2. QueryService -> IApplicationViewCollection（service 与 riid 相同）
    let view_col_iid = GUID::from_u128(IID_APP_VIEW_COLLECTION);
    let mut view_col: *mut c_void = std::ptr::null_mut();
    let hr = (sp_vtbl.query_service)(sp, &view_col_iid, &view_col_iid, &mut view_col);
    if hr.is_err() || view_col.is_null() {
        return Err(format!("QueryService(IApplicationViewCollection) 失败：{hr:?}"));
    }

    // 3. HWND -> IApplicationView
    let collection_vtbl: &IAppViewCollectionVtbl = &*(*(view_col as *const IAppViewCollection)).vtbl;
    let mut view: *mut c_void = std::ptr::null_mut();
    let hr = (collection_vtbl.get_view_for_hwnd)(view_col, hwnd, &mut view);
    if hr.is_err() || view.is_null() {
        return Err(format!("GetViewForHwnd 失败：{hr:?}"));
    }

    // 4. QueryService -> IVirtualDesktopPinnedApps
    let pinned_clsid = GUID::from_u128(CLSID_PINNED_APPS);
    let pinned_iid = GUID::from_u128(IID_PINNED_APPS);
    let mut pinned_obj: *mut c_void = std::ptr::null_mut();
    let hr = (sp_vtbl.query_service)(sp, &pinned_clsid, &pinned_iid, &mut pinned_obj);
    if hr.is_err() || pinned_obj.is_null() {
        return Err(format!("QueryService(IVirtualDesktopPinnedApps) 失败：{hr:?}"));
    }

    // 5. 仅在未固定时执行 PinView。
    let pinned_vtbl: &IPinnedAppsVtbl = &*(*(pinned_obj as *const IPinnedApps)).vtbl;
    let mut pinned_now: i32 = 0;
    let hr = (pinned_vtbl.is_view_pinned)(pinned_obj, view, &mut pinned_now);
    if hr.is_err() {
        return Err(format!("IsViewPinned 失败：{hr:?}"));
    }
    if pinned_now == 0 {
        let hr = (pinned_vtbl.pin_view)(pinned_obj, view);
        if hr.is_err() {
            return Err(format!("PinView 失败：{hr:?}"));
        }
    }
    Ok(())
}
