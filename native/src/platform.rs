//! Small platform shims for behaviour that differs per OS.

/// Open a URL or file path with the system's default handler.
///
/// Windows goes through `ShellExecuteW` rather than `cmd /C start`. Spawning
/// `cmd` from a GUI process flashes a console window, and `cmd` re-parses the
/// arguments it is handed, so a link containing `&` or `|` — which can come
/// from any vault file or PDF — would be run as a shell command.
pub fn open_external(target: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let file: Vec<u16> = OsStr::new(target)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let verb: Vec<u16> = OsStr::new("open")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // SAFETY: `verb` and `file` are NUL-terminated UTF-16 buffers that
        // outlive the call; the window handle, parameters and directory
        // arguments are all documented as optional and accept null.
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            );
        }
    }
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(target).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
}
