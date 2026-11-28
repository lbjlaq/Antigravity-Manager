use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

static SHARED_HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// 异步派生短生命周期外部命令并在后台回收退出状态，防止在 macOS / Linux 下残留 `<defunct>` 僵尸进程；
/// Windows 下自动附加 `CREATE_NO_WINDOW`，防止从 GUI 进程派生控制台子进程时弹出命令行窗口
pub fn spawn_and_reap(mut cmd: Command) {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    if let Ok(mut child) = cmd.spawn() {
        let _ = std::thread::Builder::new()
            .name("am-child-reaper".into())
            .spawn(move || {
                let _ = child.wait();
            });
    }
}

/// 进程级共享精简 HTTP 客户端：
/// - 全局复用一套 rustls 根证书存储、TLS 会话缓存与连接池（避免 OAuth/Quota/Update 各自维护多套连接池）
/// - 限制每主机最大空闲连接为 2，闲置 30 秒自动释放底层 Socket 与 TLS 缓冲区
pub fn shared_http_client() -> &'static reqwest::Client {
    SHARED_HTTP_CLIENT.get_or_init(|| {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(2)
            .pool_idle_timeout(Duration::from_secs(30));

        // 遵循操作系统系统代理设置（不扫描端口，行为与浏览器一致），避免必须开启 TUN
        if let Some(sys_proxy) = am_proxy::detect_system_proxies().into_iter().next() {
            if let Ok(proxy) = reqwest::Proxy::all(sys_proxy.to_url()) {
                builder = builder.proxy(
                    proxy.no_proxy(reqwest::NoProxy::from_string("localhost,127.0.0.1,::1")),
                );
            }
        }

        builder.build().unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// 跨平台主动向操作系统归还闲置物理内存页（用于销毁 WebView 或大日志冷扫描后消除 RSS 滞留）
/// - macOS: `malloc_zone_pressure_relief(NULL, 0)` 释放 libmalloc 闲置页
/// - Linux (glibc): `malloc_trim(0)` 释放多线程 arena 中已 `free` 但未归还 OS 的堆内存
/// - Windows: `SetProcessWorkingSetSize(GetCurrentProcess(), -1, -1)` 提示内核修剪工作集
pub fn trim_process_memory() {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
        }
        unsafe {
            let _ = malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
        }
    }

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        extern "C" {
            fn malloc_trim(pad: usize) -> std::ffi::c_int;
        }
        unsafe {
            let _ = malloc_trim(0);
        }
    }

    #[cfg(target_os = "windows")]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentProcess() -> *mut std::ffi::c_void;
            fn SetProcessWorkingSetSize(
                h_process: *mut std::ffi::c_void,
                dw_minimum_working_set_size: usize,
                dw_maximum_working_set_size: usize,
            ) -> i32;
        }
        unsafe {
            let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
        }
    }
}
