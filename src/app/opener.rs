//! Hands a URL (or a folder) to the desktop: the default browser for links,
//! the file manager for folders.
//!
//! Windows goes through `ShellExecuteW("open")`, the documented way to open
//! a URL with the user's default handler. It used to spawn `explorer.exe
//! <url>`, but Explorer parses its own command line (commas split it, `/`
//! starts a switch) and falls back to opening a File Explorer window when it
//! cannot make sense of the argument, which is what happened to links with a
//! query string such as `https://x.com/u/status/1?s=46&t=abc`.

/// Opens `target` with the system's default handler and waits for the
/// opener to hand it over (so nothing is left to reap).
pub async fn open(target: String) {
    if !is_safe_target(&target) {
        tracing::warn!("refusing to open a target with control characters");
        return;
    }
    let _ = tokio::task::spawn_blocking(move || {
        if let Err(e) = platform::open(&target) {
            tracing::warn!(error = %e, "could not open URL");
        }
    })
    .await;
}

/// A target goes to the shell only when it has no control characters: a NUL
/// would cut it short in the wide string, a newline or tab has no business
/// in a URL or a folder path.
fn is_safe_target(target: &str) -> bool {
    !target.is_empty() && !target.chars().any(char::is_control)
}

/// `s` as a NUL-terminated UTF-16 string for the wide Win32 API, or `None`
/// when it holds a NUL of its own.
#[cfg_attr(not(windows), allow(dead_code))]
fn to_wide(s: &str) -> Option<Vec<u16>> {
    if s.contains('\0') {
        return None;
    }
    Some(s.encode_utf16().chain(std::iter::once(0)).collect())
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    pub fn open(target: &str) -> std::io::Result<()> {
        let file =
            super::to_wide(target).ok_or_else(|| std::io::Error::other("target holds a NUL"))?;
        let verb =
            super::to_wide("open").ok_or_else(|| std::io::Error::other("verb holds a NUL"))?;
        // ShellExecute may hand the URL to a COM handler: Microsoft asks for
        // an STA with OLE1 DDE off. A thread already in another apartment
        // (RPC_E_CHANGED_MODE) still works; only a successful init is undone.
        // SAFETY: plain Win32 calls; both strings are NUL-terminated and
        // outlive the call, the other pointers may be null.
        let com = unsafe {
            CoInitializeEx(
                std::ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        };
        let code = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if com >= 0 {
            // SAFETY: balances the successful CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
        // Per the docs, a value above 32 is success; anything else is an
        // SE_ERR_* / ERROR_* code.
        if code > 32 {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "ShellExecuteW failed ({code})"
            )))
        }
    }
}

#[cfg(not(windows))]
mod platform {
    #[cfg(target_os = "macos")]
    const OPENER: &str = "open";
    #[cfg(not(target_os = "macos"))]
    const OPENER: &str = "xdg-open";

    pub fn open(target: &str) -> std::io::Result<()> {
        // Wait for the opener so it is reaped (xdg-open/open return as soon
        // as the browser has been handed the URL).
        std::process::Command::new(OPENER)
            .arg(target)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?
            .wait()
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_strings_are_nul_terminated_utf16() {
        let url = "https://x.com/user/status/123?s=46&t=AbC,d";
        let w = to_wide(url).unwrap();
        assert_eq!(w.last(), Some(&0));
        assert_eq!(String::from_utf16(&w[..w.len() - 1]).unwrap(), url);
        // Non-ASCII survives as UTF-16 (surrogate pair included).
        let w = to_wide("https://ja.wikipedia.org/wiki/日本😀").unwrap();
        assert_eq!(
            String::from_utf16(&w[..w.len() - 1]).unwrap(),
            "https://ja.wikipedia.org/wiki/日本😀"
        );
        assert_eq!(to_wide("a\0b"), None);
    }

    #[test]
    fn query_strings_and_punctuation_are_safe_targets() {
        for url in [
            "https://x.com/user/status/123?s=46&t=abc",
            "https://twitter.com/i/web/status/1",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL,1",
            "https://example.com/a%20b#frag",
            r"C:\Users\me\AppData\Roaming\ferrocord\plugins",
        ] {
            assert!(is_safe_target(url), "{url}");
        }
    }

    #[test]
    fn control_characters_are_refused() {
        for bad in ["", "https://a.b/\0x", "https://a.b/\nx", "https://a.b/\tx"] {
            assert!(!is_safe_target(bad), "{bad:?}");
        }
    }
}
