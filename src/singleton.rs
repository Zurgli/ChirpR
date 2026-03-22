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
