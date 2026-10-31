//! Clipboard population and paste into the still-active target. No window activation.
use flick_radar::handoff::Asset;
#[cfg(not(target_os = "windows"))]
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[cfg(not(target_os = "windows"))]
fn checked(mut command: Command) -> Result<String, String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Native desktop command timed out; check permissions and retry".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
#[cfg(not(target_os = "windows"))]
fn feed(program: &str, args: &[&str], bytes: &[u8]) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    let result = child
        .stdin
        .take()
        .ok_or("Missing clipboard input")?
        .write_all(bytes);
    let status = child.wait().map_err(|e| e.to_string())?;
    result.map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("{program} failed: {status}"));
    }
    Ok(())
}
#[cfg(target_os = "macos")]
pub fn foreground() -> Result<u64, String> {
    let mut c = Command::new("osascript");
    c.args(["-e","tell application \"System Events\" to get unix id of first application process whose frontmost is true"]);
    checked(c)?
        .parse()
        .map_err(|_| "Cannot identify target application".into())
}
#[cfg(target_os = "linux")]
pub fn foreground() -> Result<u64, String> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return Err("Automatic paste is unavailable on Wayland; use the received file/clipboard manually on X11 or a supported desktop".into());
    }
    let mut c = Command::new("xdotool");
    c.arg("getwindowfocus");
    checked(c)?
        .parse()
        .map_err(|_| "Cannot identify active X11 window".into())
}
#[cfg(target_os = "macos")]
pub fn paste(asset: &Asset, target: u64) -> Result<(), String> {
    if foreground()? != target {
        return Err("Target application changed; automatic paste cancelled".into());
    }
    match asset {
        Asset::Text(text) => feed("pbcopy", &[], text.as_bytes())?,
        Asset::File(path) => {
            let mut c = Command::new("osascript");
            c.args([
                "-e",
                "on run argv",
                "-e",
                "set the clipboard to (POSIX file (item 1 of argv)) as alias",
                "-e",
                "end run",
                "--",
            ])
            .arg(path);
            checked(c)?;
        }
    }
    if foreground()? != target {
        return Err("Target application changed; clipboard is ready for manual paste".into());
    }
    let mut c = Command::new("osascript");
    c.args([
        "-e",
        "tell application \"System Events\" to keystroke \"v\" using command down",
    ]);
    checked(c)?;
    Ok(())
}
#[cfg(target_os = "linux")]
pub fn paste(asset: &Asset, target: u64) -> Result<(), String> {
    if foreground()? != target {
        return Err("Target window changed; automatic paste cancelled".into());
    }
    match asset {
        Asset::Text(text) => feed(
            "xclip",
            &["-selection", "clipboard", "-target", "UTF8_STRING"],
            text.as_bytes(),
        )?,
        Asset::File(path) => {
            use std::os::unix::ffi::OsStrExt;
            let encoded = path
                .as_os_str()
                .as_bytes()
                .iter()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || b"/-_.~".contains(b) {
                        (*b as char).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect::<String>();
            feed(
                "xclip",
                &["-selection", "clipboard", "-target", "text/uri-list"],
                format!("file://{encoded}\r\n").as_bytes(),
            )?;
        }
    }
    if foreground()? != target {
        return Err("Target window changed; clipboard is ready for manual paste".into());
    }
    let mut c = Command::new("xdotool");
    c.args(["key", "--clearmodifiers", "ctrl+v"]);
    checked(c)?;
    Ok(())
}
#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::{ffi::c_void, mem::size_of};
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Key {
        vk: u16,
        scan: u16,
        flags: u32,
        time: u32,
        extra: usize,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Mouse {
        x: i32,
        y: i32,
        data: u32,
        flags: u32,
        time: u32,
        extra: usize,
    }
    #[repr(C)]
    union Data {
        key: Key,
        mouse: Mouse,
    }
    #[repr(C)]
    struct Input {
        kind: u32,
        data: Data,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> *mut c_void;
        fn OpenClipboard(window: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(format: u32, memory: *mut c_void) -> *mut c_void;
        fn SendInput(count: u32, inputs: *const Input, size: i32) -> u32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalAlloc(flags: u32, size: usize) -> *mut c_void;
        fn GlobalLock(memory: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(memory: *mut c_void) -> i32;
        fn GlobalFree(memory: *mut c_void) -> *mut c_void;
    }
    pub fn foreground() -> Result<u64, String> {
        let handle = unsafe { GetForegroundWindow() };
        if handle.is_null() {
            Err("No foreground target".into())
        } else {
            Ok(handle as usize as u64)
        }
    }
    pub fn paste(asset: &Asset, target: u64) -> Result<(), String> {
        if foreground()? != target {
            return Err("Target window changed; automatic paste cancelled".into());
        }
        let (format, bytes) = match asset {
            Asset::Text(text) => (
                13,
                text.encode_utf16()
                    .chain([0])
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            ),
            Asset::File(path) => {
                use std::os::windows::ffi::OsStrExt;
                // DROPFILES: 20-byte header, followed by double-NUL UTF-16 file list.
                let mut b = vec![0u8; 20];
                b[..4].copy_from_slice(&20u32.to_le_bytes());
                b[16..20].copy_from_slice(&1u32.to_le_bytes());
                b.extend(
                    path.as_os_str()
                        .encode_wide()
                        .chain([0, 0])
                        .flat_map(u16::to_le_bytes),
                );
                (15, b)
            }
        };
        unsafe {
            let memory = GlobalAlloc(2, bytes.len());
            if memory.is_null() {
                return Err("Clipboard allocation failed".into());
            }
            let pointer = GlobalLock(memory);
            if pointer.is_null() {
                GlobalFree(memory);
                return Err("Clipboard lock failed".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast(), bytes.len());
            GlobalUnlock(memory);
            // A real owner HWND is required after EmptyClipboard. The non-focusable
            // overlay window owns the clipboard without changing the target focus.
            let owner = CLIPBOARD_OWNER.load(std::sync::atomic::Ordering::Relaxed) as *mut c_void;
            if owner.is_null() || OpenClipboard(owner) == 0 {
                GlobalFree(memory);
                return Err("Clipboard busy; retry the gesture".into());
            }
            let ok = EmptyClipboard() != 0 && !SetClipboardData(format, memory).is_null();
            CloseClipboard();
            if !ok {
                GlobalFree(memory);
                return Err("Cannot populate clipboard".into());
            }
            if foreground()? != target {
                return Err("Focus changed; clipboard ready for manual paste".into());
            }
            let key = |vk, flags| Input {
                kind: 1,
                data: Data {
                    key: Key {
                        vk,
                        scan: 0,
                        flags,
                        time: 0,
                        extra: 0,
                    },
                },
            };
            let inputs = [key(0x11, 0), key(0x56, 0), key(0x56, 2), key(0x11, 2)];
            if SendInput(4, inputs.as_ptr(), size_of::<Input>() as i32) != 4 {
                let release = [key(0x56, 2), key(0x11, 2)];
                SendInput(2, release.as_ptr(), size_of::<Input>() as i32);
                return Err("Windows blocked paste injection (possibly an elevated target); clipboard is ready".into());
            }
        }
        Ok(())
    }
    pub static CLIPBOARD_OWNER: std::sync::atomic::AtomicIsize =
        std::sync::atomic::AtomicIsize::new(0);
    pub fn open_path(path: &std::path::Path) -> Result<(), String> {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn ShellExecuteW(
                hwnd: *mut c_void,
                verb: *const u16,
                file: *const u16,
                params: *const u16,
                dir: *const u16,
                show: i32,
            ) -> isize;
        }
        let file: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let verb: Vec<u16> = "open".encode_utf16().chain([0]).collect();
        let code = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        if code > 32 {
            Ok(())
        } else {
            Err(format!(
                "Windows could not open the file ({code}). It remains in the inbox."
            ))
        }
    }
}
#[cfg(target_os = "windows")]
pub use windows::{foreground, paste, CLIPBOARD_OWNER};

/// Open a received PDF, Word document, MP3, or WAV with the default application.
/// The caller has already restricted the path to one of those extensions.
#[cfg(windows)]
pub fn open_file(path: &std::path::Path) -> Result<(), String> {
    windows::open_path(path)
}
#[cfg(not(windows))]
pub fn open_file(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    let mut command = Command::new(program);
    command.arg(path);
    checked(command).map(|_| ())
}

/// Make startup failures visible in Windows GUI builds without a console.
#[cfg(windows)]
pub fn startup_error(message: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            owner: *mut std::ffi::c_void,
            text: *const u16,
            title: *const u16,
            flags: u32,
        ) -> i32;
    }
    let text: Vec<u16> = message.encode_utf16().chain([0]).collect();
    let title: Vec<u16> = "Flick could not start".encode_utf16().chain([0]).collect();
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
    }
}
#[cfg(not(windows))]
pub fn startup_error(message: &str) {
    eprintln!("{message}");
}
