#[cfg(target_os = "windows")]
use std::ffi::OsStr;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "windows")]
#[repr(C)]
struct GUID {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[cfg(target_os = "windows")]
const CLSID_SHELL_LINK: GUID = GUID {
    data1: 0x00021401,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

#[cfg(target_os = "windows")]
const IID_ISHELL_LINK_W: GUID = GUID {
    data1: 0x000214F9,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

#[cfg(target_os = "windows")]
const IID_IPERSIST_FILE: GUID = GUID {
    data1: 0x0000010b,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

#[cfg(target_os = "windows")]
#[repr(C)]
struct IShellLinkWVtbl {
    // IUnknown
    query_interface: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        riid: *const GUID,
        ppv: *mut *mut std::ffi::c_void,
    ) -> i32,
    add_ref: unsafe extern "system" fn(this: *mut std::ffi::c_void) -> u32,
    release: unsafe extern "system" fn(this: *mut std::ffi::c_void) -> u32,
    // IShellLinkW
    get_path: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        psz_file: *mut u16,
        cch: i32,
        pfd: *mut std::ffi::c_void,
        f_flags: u32,
    ) -> i32,
    get_id_list: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        ppidl: *mut *mut std::ffi::c_void,
    ) -> i32,
    set_id_list: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        pidl: *const std::ffi::c_void,
    ) -> i32,
    get_description:
        unsafe extern "system" fn(this: *mut std::ffi::c_void, psz_name: *mut u16, cch: i32) -> i32,
    set_description:
        unsafe extern "system" fn(this: *mut std::ffi::c_void, psz_name: *const u16) -> i32,
    get_working_directory:
        unsafe extern "system" fn(this: *mut std::ffi::c_void, psz_dir: *mut u16, cch: i32) -> i32,
    set_working_directory:
        unsafe extern "system" fn(this: *mut std::ffi::c_void, psz_dir: *const u16) -> i32,
    get_arguments:
        unsafe extern "system" fn(this: *mut std::ffi::c_void, psz_args: *mut u16, cch: i32) -> i32,
    set_arguments:
        unsafe extern "system" fn(this: *mut std::ffi::c_void, psz_args: *const u16) -> i32,
    get_hotkey: unsafe extern "system" fn(this: *mut std::ffi::c_void, pw_hotkey: *mut u16) -> i32,
    set_hotkey: unsafe extern "system" fn(this: *mut std::ffi::c_void, w_hotkey: u16) -> i32,
    get_show_cmd:
        unsafe extern "system" fn(this: *mut std::ffi::c_void, pi_show_cmd: *mut i32) -> i32,
    set_show_cmd: unsafe extern "system" fn(this: *mut std::ffi::c_void, i_show_cmd: i32) -> i32,
    get_icon_location: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        psz_icon_path: *mut u16,
        cch: i32,
        pi_icon: *mut i32,
    ) -> i32,
    set_icon_location: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        psz_icon_path: *const u16,
        i_icon: i32,
    ) -> i32,
    set_relative_path: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        psz_path_rel: *const u16,
        dw_reserved: u32,
    ) -> i32,
    resolve: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        hwnd: *mut std::ffi::c_void,
        f_flags: u32,
    ) -> i32,
    set_path: unsafe extern "system" fn(this: *mut std::ffi::c_void, psz_file: *const u16) -> i32,
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct IPersistFileVtbl {
    // IUnknown
    query_interface: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        riid: *const GUID,
        ppv: *mut *mut std::ffi::c_void,
    ) -> i32,
    add_ref: unsafe extern "system" fn(this: *mut std::ffi::c_void) -> u32,
    release: unsafe extern "system" fn(this: *mut std::ffi::c_void) -> u32,
    // IPersist
    get_class_id:
        unsafe extern "system" fn(this: *mut std::ffi::c_void, p_class_id: *mut GUID) -> i32,
    // IPersistFile
    is_dirty: unsafe extern "system" fn(this: *mut std::ffi::c_void) -> i32,
    load: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        psz_file_name: *const u16,
        dw_mode: u32,
    ) -> i32,
    save: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        psz_file_name: *const u16,
        f_remember: i32,
    ) -> i32,
    save_completed:
        unsafe extern "system" fn(this: *mut std::ffi::c_void, psz_file_name: *const u16) -> i32,
    get_cur_file: unsafe extern "system" fn(
        this: *mut std::ffi::c_void,
        ppsz_file_name: *mut *mut u16,
    ) -> i32,
}

#[cfg(target_os = "windows")]
#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(pv_reserved: *mut std::ffi::c_void, dw_co_init: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(
        rclsid: *const GUID,
        p_unk_outer: *mut std::ffi::c_void,
        dw_cls_context: u32,
        riid: *const GUID,
        ppv: *mut *mut std::ffi::c_void,
    ) -> i32;
}

#[cfg(target_os = "windows")]
#[link(name = "shell32")]
extern "system" {
    fn SHChangeNotify(
        w_event_id: i32,
        u_flags: u32,
        dw_item1: *const std::ffi::c_void,
        dw_item2: *const std::ffi::c_void,
    );
}

/// 纯原生 Win32 COM 实现快捷方式图标自愈与 Shell 刷新（零外部进程，不调用 powershell.exe）
#[cfg(target_os = "windows")]
pub fn heal_shortcuts_native() {
    let mut target_dirs = Vec::new();
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        target_dirs.push(PathBuf::from(&userprofile).join("Desktop"));
    }
    if let Ok(public) = std::env::var("PUBLIC") {
        target_dirs.push(PathBuf::from(&public).join("Desktop"));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        target_dirs.push(PathBuf::from(&appdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(programdata) = std::env::var("ProgramData") {
        target_dirs
            .push(PathBuf::from(&programdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }

    let mut changed = false;
    unsafe {
        let _ = CoInitializeEx(
            std::ptr::null_mut(),
            0x0, /* COINIT_MULTITHREADED / APARTMENT */
        );
    }

    for base_dir in target_dirs {
        if !base_dir.exists() {
            continue;
        }
        scan_and_heal_dir(&base_dir, &mut changed);
    }

    unsafe {
        if changed {
            // SHCNE_ASSOCCHANGED = 0x08000000, SHCNF_IDLIST = 0x0000
            SHChangeNotify(0x08000000, 0, std::ptr::null(), std::ptr::null());
        }
        CoUninitialize();
    }
}

#[cfg(target_os = "windows")]
fn scan_and_heal_dir(dir: &Path, changed: &mut bool) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_and_heal_dir(&path, changed);
            } else if path.is_file() {
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if file_name.to_lowercase().contains("antigravity")
                        && file_name.to_lowercase().ends_with(".lnk")
                    {
                        if heal_single_shortcut(&path) {
                            *changed = true;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn heal_single_shortcut(lnk_path: &Path) -> bool {
    unsafe {
        let mut shell_link_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        if CoCreateInstance(
            &CLSID_SHELL_LINK,
            std::ptr::null_mut(),
            1, // CLSCTX_INPROC_SERVER
            &IID_ISHELL_LINK_W,
            &mut shell_link_ptr,
        ) != 0
            || shell_link_ptr.is_null()
        {
            return false;
        }

        let sl_vtbl = *(shell_link_ptr as *mut *const IShellLinkWVtbl);
        let mut persist_file_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        if ((*sl_vtbl).query_interface)(shell_link_ptr, &IID_IPERSIST_FILE, &mut persist_file_ptr)
            != 0
            || persist_file_ptr.is_null()
        {
            ((*sl_vtbl).release)(shell_link_ptr);
            return false;
        }
        let pf_vtbl = *(persist_file_ptr as *mut *const IPersistFileVtbl);

        let mut lnk_path_u16: Vec<u16> = lnk_path.as_os_str().encode_wide().collect();
        lnk_path_u16.push(0);

        // STGM_READWRITE = 0x00000002
        if ((*pf_vtbl).load)(persist_file_ptr, lnk_path_u16.as_ptr(), 2) != 0 {
            ((*pf_vtbl).release)(persist_file_ptr);
            ((*sl_vtbl).release)(shell_link_ptr);
            return false;
        }

        let mut target_buf = [0u16; 512];
        let _ = ((*sl_vtbl).get_path)(
            shell_link_ptr,
            target_buf.as_mut_ptr(),
            512,
            std::ptr::null_mut(),
            0,
        );

        let mut icon_buf = [0u16; 512];
        let mut icon_idx = 0i32;
        let _ = ((*sl_vtbl).get_icon_location)(
            shell_link_ptr,
            icon_buf.as_mut_ptr(),
            512,
            &mut icon_idx,
        );

        let icon_str = String::from_utf16_lossy(&icon_buf)
            .trim_matches('\0')
            .trim()
            .to_string();
        let target_str = String::from_utf16_lossy(&target_buf)
            .trim_matches('\0')
            .trim()
            .to_string();

        let mut updated = false;
        if (icon_str.is_empty() || icon_str == ",0")
            && !target_str.is_empty()
            && target_str.to_lowercase().ends_with(".exe")
        {
            let mut target_u16: Vec<u16> = OsStr::new(&target_str).encode_wide().collect();
            target_u16.push(0);
            if ((*sl_vtbl).set_icon_location)(shell_link_ptr, target_u16.as_ptr(), 0) == 0 {
                if ((*pf_vtbl).save)(persist_file_ptr, lnk_path_u16.as_ptr(), 1) == 0 {
                    updated = true;
                }
            }
        }

        ((*pf_vtbl).release)(persist_file_ptr);
        ((*sl_vtbl).release)(shell_link_ptr);
        updated
    }
}
