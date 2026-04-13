use anyhow::{Result, bail};

#[cfg(target_os = "windows")]
pub struct WindowsMutexGuard {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(not(target_os = "windows"))]
pub struct WindowsMutexGuard;

#[cfg(target_os = "windows")]
impl Drop for WindowsMutexGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = windows_sys::Win32::System::Threading::ReleaseMutex(self.handle);
            let _ = windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(not(target_os = "windows"))]
impl Drop for WindowsMutexGuard {
    fn drop(&mut self) {}
}

pub fn acquire_named_mutex(name: &str, already_running_message: &str) -> Result<WindowsMutexGuard> {
    match try_acquire_named_mutex(name)? {
        Some(guard) => Ok(guard),
        None => bail!("{already_running_message}"),
    }
}

pub fn try_acquire_named_mutex(name: &str) -> Result<Option<WindowsMutexGuard>> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, SetLastError};
        use windows_sys::Win32::System::Threading::CreateMutexW;

        let name = wide(name);
        // CreateMutexW only sets GetLastError to ERROR_ALREADY_EXISTS when opening an existing
        // mutex; otherwise it may leave a stale code from an unrelated prior API call. Without
        // clearing first, we can falsely close a brand-new handle and never acquire the mutex.
        unsafe { SetLastError(0) };
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
        if handle.is_null() {
            bail!("failed to create Windows mutex");
        }

        let last_error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        const ERROR_ALREADY_EXISTS: u32 = 183;
        if last_error == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Ok(None);
        }

        return Ok(Some(WindowsMutexGuard { handle }));
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = name;
        Ok(Some(WindowsMutexGuard))
    }
}

#[cfg(target_os = "windows")]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Keep in sync with `SETTINGS_WINDOW_TITLE` in `settings.rs`.
#[cfg(target_os = "windows")]
fn chirp_settings_window_owner_pid() -> Option<u32> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};

    const SETTINGS_TITLE: &str = "ChirpR Settings";
    let title = wide(SETTINGS_TITLE);
    let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    if hwnd == HWND::default() {
        return None;
    }
    let mut pid: u32 = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
    }
    if pid == 0 {
        None
    } else {
        Some(pid)
    }
}

#[cfg(target_os = "windows")]
pub fn focus_window_by_class(class_name: &str) -> Result<bool> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };

    let class_name = wide(class_name);
    let hwnd = unsafe { FindWindowW(class_name.as_ptr(), std::ptr::null()) };
    if hwnd == HWND::default() {
        return Ok(false);
    }

    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }

    Ok(true)
}

#[cfg(not(target_os = "windows"))]
pub fn focus_window_by_class(_class_name: &str) -> Result<bool> {
    Ok(false)
}

#[cfg(target_os = "windows")]
pub fn focus_window_by_title(window_title: &str) -> Result<bool> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };

    let window_title = wide(window_title);
    let hwnd = unsafe { FindWindowW(std::ptr::null(), window_title.as_ptr()) };
    if hwnd == HWND::default() {
        return Ok(false);
    }

    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }

    Ok(true)
}

#[cfg(not(target_os = "windows"))]
pub fn focus_window_by_title(_window_title: &str) -> Result<bool> {
    Ok(false)
}

#[cfg(target_os = "windows")]
pub fn terminate_other_app_instances() -> Result<()> {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
    };

    fn utf16z_to_string(buf: &[u16]) -> String {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }

    fn query_process_image_path(process: HANDLE) -> Option<PathBuf> {
        let mut size = 260_u32;
        let mut buffer = vec![0_u16; size as usize];
        loop {
            let mut needed = size;
            let ok =
                unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut needed) };
            if ok != 0 {
                return Some(PathBuf::from(String::from_utf16_lossy(
                    &buffer[..needed as usize],
                )));
            }

            const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
            let error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            if error != ERROR_INSUFFICIENT_BUFFER {
                return None;
            }

            size *= 2;
            buffer.resize(size as usize, 0);
        }
    }

    /// `cargo run` uses `chirpr-cli.exe` while the tray/settings launcher is `chirpr.exe`; both
    /// share the app singleton. Dev runner also copies binaries into `target/dev-runner/` under
    /// staged names (`chirpr-cli-child-*.exe`, etc.).
    fn is_chirp_app_executable(image: &Path) -> bool {
        let Some(name) = image
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_ascii_lowercase())
        else {
            return false;
        };
        if matches!(name.as_str(), "chirpr.exe" | "chirpr-cli.exe") {
            return true;
        }
        let Some(parent) = image
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
        else {
            return false;
        };
        if !parent.eq_ignore_ascii_case("dev-runner") {
            return false;
        }
        name.starts_with("chirpr-cli-") || name.starts_with("chirpr-")
    }

    fn cargo_target_directory(executable: &Path) -> Option<PathBuf> {
        executable.parent()?.ancestors().find_map(|ancestor| {
            let name = ancestor.file_name().and_then(|n| n.to_str())?;
            if name.eq_ignore_ascii_case("target") || name.eq_ignore_ascii_case("cargo-target") {
                Some(ancestor.to_path_buf())
            } else {
                None
            }
        })
    }

    fn same_chirp_install(candidate_image: &Path, current_image: &Path) -> bool {
        match (
            candidate_image.parent(),
            current_image.parent(),
        ) {
            (Some(left), Some(right)) if left == right => return true,
            _ => {}
        }
        match (
            cargo_target_directory(candidate_image),
            cargo_target_directory(current_image),
        ) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        }
    }

    /// True when both images live under the same Cargo package root (directory containing
    /// `Cargo.toml`). Used when `target/` and `cargo-target/` (sandbox) layouts differ only by
    /// leaf name so we still match `chirpr.exe` and `chirpr-cli.exe` from one workspace.
    fn same_cargo_package_root(candidate_image: &Path, current_image: &Path) -> bool {
        fn package_root(executable: &Path) -> Option<PathBuf> {
            let mut dir = executable.parent()?;
            loop {
                if dir.join("Cargo.toml").is_file() {
                    return Some(dir.to_path_buf());
                }
                dir = dir.parent()?;
            }
        }

        match (
            package_root(candidate_image),
            package_root(current_image),
        ) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        }
    }

    fn snapshot_name_might_be_chirp_app(exe_name: &str) -> bool {
        exe_name == "chirpr.exe"
            || exe_name == "chirpr-cli.exe"
            || exe_name.starts_with("chirpr-cli-")
            || exe_name.starts_with("chirpr-")
    }

    let current_pid = std::process::id();
    let current_exe = std::env::current_exe()?;
    let settings_owner_pid = chirp_settings_window_owner_pid();

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        bail!("failed to enumerate running processes");
    }

    let mut targets: BTreeSet<u32> = BTreeSet::new();
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while has_entry {
        let pid = entry.th32ProcessID;
        if pid != current_pid && Some(pid) != settings_owner_pid {
            let exe_name = utf16z_to_string(&entry.szExeFile).to_ascii_lowercase();
            if snapshot_name_might_be_chirp_app(&exe_name) {
                let process = unsafe {
                    OpenProcess(
                        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                        0,
                        pid,
                    )
                };

                if !process.is_null() {
                    if let Some(image_path) = query_process_image_path(process) {
                        if is_chirp_app_executable(&image_path)
                            && (same_chirp_install(&image_path, &current_exe)
                                || same_cargo_package_root(&image_path, &current_exe))
                        {
                            targets.insert(pid);
                        }
                    }

                    unsafe {
                        CloseHandle(process);
                    }
                }
            }
        }

        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }

    unsafe {
        CloseHandle(snapshot);
    }

    let had_targets = !targets.is_empty();
    for pid in targets {
        let process = unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid) };

        if process.is_null() {
            continue;
        }

        unsafe {
            let _ = TerminateProcess(process, 0);
        }

        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let wait_result = unsafe { WaitForSingleObject(process, 100) };
            if wait_result == 0 {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }

        unsafe {
            CloseHandle(process);
        }
    }

    if had_targets {
        // Let the singleton mutex and other handles drain before a replacement process starts.
        thread::sleep(Duration::from_millis(200));
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn terminate_other_app_instances() -> Result<()> {
    Ok(())
}
