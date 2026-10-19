//! Windows GUI 子系统的 CLI 控制台连接；保留调用方的输出重定向。
//!
//! @author ljlVink

#[cfg(windows)]
pub(super) fn attach() {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn GetStdHandle(kind: u32) -> *mut c_void;
        fn SetStdHandle(kind: u32, handle: *mut c_void) -> i32;
        fn GetFileType(handle: *mut c_void) -> u32;
    }

    // 只在 CLI 分支连接已有控制台，不分配新窗口。AttachConsole 可能替换
    // 标准句柄，因此保留父进程传入的文件/管道，避免破坏重定向。
    // https://learn.microsoft.com/en-us/windows/console/getstdhandle
    unsafe {
        let redirected = [-10_i32, -11, -12].map(|kind| {
            let handle = GetStdHandle(kind as u32);
            let keep =
                !handle.is_null() && handle as isize != -1 && matches!(GetFileType(handle), 1 | 3);
            (kind as u32, handle, keep)
        });
        if AttachConsole(u32::MAX) != 0 {
            for (kind, handle, keep) in redirected {
                if keep {
                    SetStdHandle(kind, handle);
                }
            }
        }
    }
}

#[cfg(not(windows))]
pub(super) fn attach() {}
