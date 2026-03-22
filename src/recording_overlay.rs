use std::iter;
use std::ptr;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW,
    Ellipse, EndPaint, FillRect, InvalidateRect, PAINTSTRUCT, SetBkMode, SetTextColor, TRANSPARENT,
    UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetClientRect, GetMessageW, GetSystemMetrics, MSG, PostMessageW, PostQuitMessage,
    RegisterClassW, SM_CXSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, SetProcessDPIAware, ShowWindow,
    TranslateMessage, WM_APP, WM_CLOSE, WM_DESTROY, WM_PAINT, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const WM_APP_SHOW: u32 = WM_APP + 1;
const WM_APP_HIDE: u32 = WM_APP + 2;
const WM_APP_CLOSE: u32 = WM_APP + 3;
const WM_APP_SET_MODE: u32 = WM_APP + 4;

static CLASS_NAME: &str = "ChirpRustRecordingOverlay";
static OVERLAY_STATE: OnceLock<Arc<Mutex<OverlayState>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayGeometry {
    pub width: i32,
    pub height: i32,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone)]
struct OverlayState {
    label: String,
}

pub struct RecordingOverlay {
    enabled: bool,
    hwnd: Arc<Mutex<isize>>,
    state: Arc<Mutex<OverlayState>>,
}

impl RecordingOverlay {
    pub fn new(enabled: bool) -> Self {
        if !enabled || !cfg!(target_os = "windows") {
            return Self {
                enabled: false,
                hwnd: Arc::new(Mutex::new(0)),
                state: Arc::new(Mutex::new(OverlayState {
                    label: "Transcribing".to_string(),
                })),
            };
        }

        let hwnd = Arc::new(Mutex::new(0_isize));
        let state = Arc::new(Mutex::new(OverlayState {
            label: "Transcribing".to_string(),
        }));
        let _ = OVERLAY_STATE.set(Arc::clone(&state));
        let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));

        spawn_overlay_thread(Arc::clone(&hwnd), Arc::clone(&state), Arc::clone(&ready));
        for _ in 0..200 {
            if ready.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        Self {
            enabled: true,
            hwnd,
            state,
        }
    }

    pub fn show(&self, mode: &str) {
        if !self.enabled {
            return;
        }

        self.set_mode(mode);
        let hwnd = self.window_handle();
        if hwnd != 0 {
            unsafe {
                let _ = PostMessageW(hwnd as HWND, WM_APP_SHOW, 0, 0);
            }
        }
    }

    pub fn hide(&self) {
        if !self.enabled {
            return;
        }

        let hwnd = self.window_handle();
        if hwnd != 0 {
            unsafe {
                let _ = PostMessageW(hwnd as HWND, WM_APP_HIDE, 0, 0);
            }
        }
    }

    pub fn close(&self) {
        if !self.enabled {
            return;
        }

        let hwnd = self.window_handle();
        if hwnd != 0 {
            unsafe {
                let _ = PostMessageW(hwnd as HWND, WM_APP_CLOSE, 0, 0);
            }
        }
    }

    pub fn set_mode(&self, mode: &str) {
        if !self.enabled {
            return;
        }

        let label = if mode == "loading" {
            "Loading model"
        } else {
            "Transcribing"
        };

        if let Ok(mut state) = self.state.lock() {
            state.label = label.to_string();
        }

        let hwnd = self.window_handle();
        if hwnd != 0 {
            unsafe {
                let _ = PostMessageW(hwnd as HWND, WM_APP_SET_MODE, 0, 0);
            }
        }
    }

    fn window_handle(&self) -> isize {
        self.hwnd.lock().map(|value| *value).unwrap_or(0)
    }
}

fn spawn_overlay_thread(
    hwnd_slot: Arc<Mutex<isize>>,
    _state: Arc<Mutex<OverlayState>>,
    ready: Arc<std::sync::atomic::AtomicBool>,
) {
    thread::spawn(move || unsafe {
        let class_name = widestr(CLASS_NAME);
        let h_instance = GetModuleHandleW(ptr::null());
        let wnd_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: h_instance,
            lpszClassName: class_name.as_ptr(),
            ..std::mem::zeroed()
        };
        let _ = RegisterClassW(&wnd_class);

        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let geometry = compute_top_center_geometry(screen_width, 168, 30, 0);
        let title = widestr("Chirp Rust");
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            geometry.x,
            geometry.y,
            geometry.width,
            geometry.height,
            ptr::null_mut(),
            ptr::null_mut(),
            h_instance,
            ptr::null(),
        );

        if let Ok(mut slot) = hwnd_slot.lock() {
            *slot = hwnd as isize;
        }
        ready.store(true, std::sync::atomic::Ordering::SeqCst);

        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    });
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match message {
        WM_APP_SET_MODE => {
            unsafe {
                let _ = InvalidateRect(hwnd, ptr::null(), 1);
                let _ = UpdateWindow(hwnd);
            }
            0
        }
        WM_APP_SHOW => {
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = InvalidateRect(hwnd, ptr::null(), 1);
                let _ = UpdateWindow(hwnd);
            }
            0
        }
        WM_APP_HIDE => {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            0
        }
        WM_APP_CLOSE | WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            0
        }
        WM_PAINT => {
            paint_overlay(hwnd);
            0
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, w_param, l_param) },
    }
}

fn paint_overlay(hwnd: HWND) {
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rect: RECT = std::mem::zeroed();
        let _ = GetClientRect(hwnd, &mut rect);

        let background = CreateSolidBrush(rgb(245, 245, 247));
        let _ = FillRect(hdc, &rect, background);
        let _ = DeleteObject(background as _);

        let dot_brush = CreateSolidBrush(rgb(255, 59, 48));
        let old_brush = windows_sys::Win32::Graphics::Gdi::SelectObject(hdc, dot_brush as _);
        let _ = Ellipse(hdc, 16, 10, 22, 16);
        let _ = windows_sys::Win32::Graphics::Gdi::SelectObject(hdc, old_brush);
        let _ = DeleteObject(dot_brush as _);

        let _ = SetBkMode(hdc, TRANSPARENT as i32);
        let _ = SetTextColor(hdc, rgb(17, 17, 17) as COLORREF);
        let mut text_rect = RECT {
            left: 30,
            top: 0,
            right: rect.right - 10,
            bottom: rect.bottom,
        };

        let text = current_label();
        let _ = DrawTextW(
            hdc,
            text.as_ptr(),
            -1,
            &mut text_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );

        EndPaint(hwnd, &ps);
    }
}

fn current_label() -> Vec<u16> {
    let label = OVERLAY_STATE
        .get()
        .and_then(|state| state.lock().ok().map(|value| value.label.clone()))
        .unwrap_or_else(|| "Transcribing".to_string());
    widestr(&label)
}

fn widestr(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(iter::once(0)).collect()
}

const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
}

pub fn compute_top_center_geometry(
    screen_width: i32,
    width: i32,
    height: i32,
    top_margin: i32,
) -> OverlayGeometry {
    let x = ((screen_width - width) / 2).max(0);
    let y = top_margin.max(0);
    OverlayGeometry {
        width,
        height,
        x,
        y,
    }
}

pub fn enable_dpi_awareness() {
    #[cfg(target_os = "windows")]
    unsafe {
        let _ = SetProcessDPIAware();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_top_center_geometry() {
        let geometry = compute_top_center_geometry(1920, 168, 30, 0);
        assert_eq!(
            geometry,
            OverlayGeometry {
                width: 168,
                height: 30,
                x: 876,
                y: 0,
            }
        );
    }
}
