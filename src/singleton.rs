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
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::CreateMutexW;

        let name = wide(name);
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            bail!("failed to create Windows mutex");
        }

        let last_error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        const ERROR_ALREADY_EXISTS: u32 = 183;
        if last_error == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            bail!("{already_running_message}");
        }

        return Ok(WindowsMutexGuard { handle });
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = name;
        let _ = already_running_message;
        Ok(WindowsMutexGuard)
    }
}

#[cfg(target_os = "windows")]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
pub fn terminate_other_app_instances() -> Result<()> {
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
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
            let ok = unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut needed) };
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

    let current_pid = std::process::id();
    let current_exe = std::env::current_exe()?;
    let current_name = current_exe
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("failed to resolve current executable name"))?
        .to_ascii_lowercase();
    let current_dir = current_exe.parent().map(Path::to_path_buf);

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        bail!("failed to enumerate running processes");
    }

    let mut targets: Vec<u32> = Vec::new();
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while has_entry {
        let pid = entry.th32ProcessID;
        if pid != current_pid {
            let exe_name = utf16z_to_string(&entry.szExeFile).to_ascii_lowercase();
            if exe_name == current_name {
                let process = unsafe {
                    OpenProcess(
                        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                        0,
                        pid,
                    )
                };

                if !process.is_null() {
                    let same_install = query_process_image_path(process)
                        .and_then(|path| path.parent().map(Path::to_path_buf))
                        .zip(current_dir.as_ref())
                        .map(|(left, right)| left == *right)
                        .unwrap_or(false);

                    if same_install {
                        targets.push(pid);
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

    for pid in targets {
        let process = unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };

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

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn terminate_other_app_instances() -> Result<()> {
    Ok(())
}
