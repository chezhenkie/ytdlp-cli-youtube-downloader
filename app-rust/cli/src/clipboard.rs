//! System clipboard read for the FR-17 quick-paste row: one function,
//! no state. Windows reads the Win32 clipboard (CF_UNICODETEXT) through
//! windows-sys; posix asks the first of wl-paste/xclip/xsel/pbpaste that
//! is on PATH and answers. None on every failure - the menu reports it.

/// The clipboard's text, or None when there is none: a refused
/// clipboard open, non-text data or empty text.
#[cfg(windows)]
pub fn read_clipboard() -> Option<String> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    // CF_UNICODETEXT
    const FORMAT: u32 = 13;
    // another app may hold the clipboard open for a moment
    const ATTEMPTS: u32 = 3;
    // SAFETY: every handle is checked before use, the text length is
    // read from GlobalSize, and the clipboard is closed on every path.
    unsafe {
        if IsClipboardFormatAvailable(FORMAT) == 0 {
            return None;
        }
        let mut opened = false;
        for attempt in 0..ATTEMPTS {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                opened = true;
                break;
            }
            if attempt + 1 < ATTEMPTS {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        if !opened {
            return None;
        }
        let handle = GetClipboardData(FORMAT);
        let mut text = None;
        if !handle.is_null() {
            let ptr = GlobalLock(handle) as *const u16;
            if !ptr.is_null() {
                let max = GlobalSize(handle) / 2;
                let mut len = 0usize;
                while len < max && ptr.add(len).read() != 0 {
                    len += 1;
                }
                text = Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                    ptr, len,
                )));
                GlobalUnlock(handle);
            }
        }
        CloseClipboard();
        text
    }
}

/// posix: the clipboard reader tools in preference order (wayland, X11,
/// X11, macOS); the first one present on PATH that exits 0 with
/// non-empty output wins.
#[cfg(unix)]
pub fn read_clipboard() -> Option<String> {
    const ATTEMPTS: [(&str, &[&str]); 4] = [
        ("wl-paste", &["--no-newline"]),
        ("xclip", &["-selection", "clipboard", "-o"]),
        ("xsel", &["--clipboard", "--output"]),
        ("pbpaste", &[]),
    ];
    for (program, args) in ATTEMPTS {
        if crate::resolve::which_from_process(program).is_none() {
            continue;
        }
        let Ok(out) = std::process::Command::new(program).args(args).output() else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}
