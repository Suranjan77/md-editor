//! Small platform shims for behaviour that differs per OS.

#![deny(clippy::undocumented_unsafe_blocks)]

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

/// Reveal `path` in the system file manager (selecting it or opening its parent folder).
pub fn reveal_in_file_manager(path: &std::path::Path) {
    let folder = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    open_external(&folder.to_string_lossy());
}

/// Detect whether the OS desktop session requests reduced animations.
///
/// Linux inspects GTK 4 / GTK 3 `settings.ini` (`gtk-enable-animations`);
/// Windows queries `SPI_GETCLIENTAREAANIMATION`; macOS checks `MD_EDITOR_REDUCE_MOTION`
/// or defaults off without spawning subprocesses during boot.
pub fn os_prefers_reduced_motion() -> bool {
    if let Ok(val) = std::env::var("MD_EDITOR_REDUCE_MOTION") {
        let low = val.trim().to_ascii_lowercase();
        if matches!(low.as_str(), "1" | "true" | "on") {
            return true;
        }
        if matches!(low.as_str(), "0" | "false" | "off") {
            return false;
        }
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
        };
        let mut enabled: i32 = 1;
        // SAFETY: `enabled` is a valid aligned `BOOL` (`i32`) buffer matching
        // `SPI_GETCLIENTAREAANIMATION`'s documented `pvParam` layout.
        let ok = unsafe {
            SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut enabled).cast(), 0)
        };
        if ok != 0 {
            return enabled == 0;
        }
    }
    #[cfg(target_os = "linux")]
    {
        let config_base = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .map(|h| h.join(".config"))
            });
        if let Some(base) = config_base {
            for rel in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
                if let Ok(contents) = std::fs::read_to_string(base.join(rel)) {
                    for line in contents.lines() {
                        if let Some((k, v)) = line.split_once('=')
                            && k.trim() == "gtk-enable-animations"
                        {
                            let v = v.trim().to_ascii_lowercase();
                            return matches!(v.as_str(), "0" | "false");
                        }
                    }
                }
            }
        }
    }
    false
}

/// What the clipboard holds, as far as pasting into a note cares.
#[derive(Clone, PartialEq, Eq)]
pub enum ClipboardPayload {
    /// A bitmap (a screenshot, a copied image), as RGBA.
    Image {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    /// Files copied in a file manager, or pasted text that names existing
    /// files (`file://` URIs or paths).
    Files(Vec<std::path::PathBuf>),
    /// Anything else: let the ordinary text paste handle it.
    Other,
}

// Hand-written so formatting a message never walks a bitmap.
impl std::fmt::Debug for ClipboardPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Image { width, height, .. } => write!(f, "Image({width}x{height})"),
            Self::Files(files) => f.debug_tuple("Files").field(files).finish(),
            Self::Other => f.write_str("Other"),
        }
    }
}

/// Read the clipboard for a paste. Blocking (it may wait on the clipboard
/// owner), so callers run it in a task.
pub fn read_clipboard() -> ClipboardPayload {
    #[cfg(test)]
    if let Some(payload) = test_clipboard::take() {
        return payload;
    }
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        return ClipboardPayload::Other;
    };
    if let Ok(image) = clipboard.get_image() {
        return ClipboardPayload::Image {
            width: image.width as u32,
            height: image.height as u32,
            rgba: image.bytes.into_owned(),
        };
    }
    if let Ok(files) = clipboard.get().file_list()
        && !files.is_empty()
    {
        return ClipboardPayload::Files(files);
    }
    match clipboard.get_text() {
        Ok(text) => files_named_by(&text).map_or(ClipboardPayload::Other, ClipboardPayload::Files),
        Err(_) => ClipboardPayload::Other,
    }
}

/// When every non-empty line of `text` is a `file://` URI or an absolute
/// path to an existing file (`text/uri-list`, or paths copied as text), those
/// files.
pub fn files_named_by(text: &str) -> Option<Vec<std::path::PathBuf>> {
    let files: Option<Vec<std::path::PathBuf>> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let path = match line.strip_prefix("file://") {
                Some(rest) => std::path::PathBuf::from(percent_decode(rest)),
                None => std::path::PathBuf::from(line),
            };
            (path.is_absolute() && path.is_file()).then_some(path)
        })
        .collect();
    files.filter(|f| !f.is_empty())
}

/// Decode `%XX` escapes (as in `file://` URIs).
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests;

/// Lets tests put something on the "clipboard" without touching the OS one.
#[cfg(test)]
pub mod test_clipboard {
    use super::ClipboardPayload;
    use std::sync::Mutex;

    static NEXT: Mutex<Option<ClipboardPayload>> = Mutex::new(None);

    pub fn set(payload: ClipboardPayload) {
        *NEXT.lock().unwrap_or_else(|p| p.into_inner()) = Some(payload);
    }

    pub(super) fn take() -> Option<ClipboardPayload> {
        NEXT.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
}
