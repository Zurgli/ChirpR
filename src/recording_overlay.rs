use std::iter;
use std::ptr;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateRoundRectRgn, CreateSolidBrush, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
    DeleteObject, DrawTextW, Ellipse, EndPaint, FillRect, InvalidateRect, PAINTSTRUCT, RoundRect,
    SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT, UpdateWindow,
};
use windows_sys::Win32::Graphics::GdiPlus::{
    CompositingQualityHighQuality, FillModeAlternate, GdipAddPathArc, GdipAddPathLine,
    GdipClosePathFigure, GdipCreateFromHDC, GdipCreatePath, GdipCreatePen1, GdipCreateSolidFill,
    GdipDeleteBrush, GdipDeleteGraphics, GdipDeletePath, GdipDeletePen, GdipDrawPath,
    GdipFillEllipseI, GdipFillPath, GdipGraphicsClear, GdipSetCompositingQuality,
    GdipSetPixelOffsetMode, GdipSetSmoothingMode, GdipSetTextRenderingHint, GdiplusStartup,
    GdiplusStartupInput, GpBrush, GpGraphics, GpPath, GpPen, LineJoinRound, PixelOffsetModeHalf,
    SmoothingModeAntiAlias, TextRenderingHintClearTypeGridFit, UnitPixel,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, GetDpiForWindow,
    SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetClientRect, GetMessageW, GetSystemMetrics, KillTimer, MSG, PostMessageW, PostQuitMessage,
    RegisterClassW, SM_CXSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER,
    SetProcessDPIAware, SetTimer, SetWindowPos, ShowWindow, TranslateMessage, WM_APP, WM_CLOSE,
    WM_DESTROY, WM_DPICHANGED, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const WM_APP_SHOW: u32 = WM_APP + 1;
const WM_APP_HIDE: u32 = WM_APP + 2;
const WM_APP_CLOSE: u32 = WM_APP + 3;
const WM_APP_SET_MODE: u32 = WM_APP + 4;
const PULSE_TIMER_ID: usize = 1;
const PULSE_INTERVAL_MS: u32 = 140;
const BASE_OVERLAY_WIDTH: i32 = 156;
const BASE_OVERLAY_HEIGHT: i32 = 24;
const BASE_TOP_MARGIN: i32 = 0;
const BASE_CORNER_RADIUS: i32 = 6;
const BASE_DOT_LEFT: i32 = 12;
const BASE_DOT_SIZE: i32 = 5;
const BASE_TEXT_LEFT: i32 = 24;
const BASE_TEXT_RIGHT: i32 = 8;

static CLASS_NAME: &str = "ChirpRustRecordingOverlay";
static OVERLAY_STATE: OnceLock<Arc<Mutex<OverlayState>>> = OnceLock::new();
static GDIPLUS_TOKEN: OnceLock<Option<usize>> = OnceLock::new();

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
    visible: bool,
    pulse_started_at: Instant,
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
                    visible: false,
                    pulse_started_at: Instant::now(),
                })),
            };
        }

        let hwnd = Arc::new(Mutex::new(0_isize));
        let state = Arc::new(Mutex::new(OverlayState {
            label: "Transcribing".to_string(),
            visible: false,
            pulse_started_at: Instant::now(),
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
        if let Ok(mut state) = self.state.lock() {
            if !state.visible {
                state.pulse_started_at = Instant::now();
            }
            state.visible = true;
        }
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

        if let Ok(mut state) = self.state.lock() {
            state.visible = false;
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

        let dpi = current_dpi(ptr::null_mut());
        let screen_width = screen_width_for_dpi(dpi);
        let geometry = overlay_geometry_for_dpi(screen_width, dpi);
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

        if !hwnd.is_null() {
            apply_overlay_region(
                hwnd,
                geometry.width,
                geometry.height,
                scale_i32(BASE_CORNER_RADIUS, dpi),
            );
        }

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
                let _ = InvalidateRect(hwnd, ptr::null(), 0);
                let _ = UpdateWindow(hwnd);
            }
            0
        }
        WM_APP_SHOW => {
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = SetTimer(hwnd, PULSE_TIMER_ID, PULSE_INTERVAL_MS, None);
                let _ = InvalidateRect(hwnd, ptr::null(), 0);
                let _ = UpdateWindow(hwnd);
            }
            0
        }
        WM_APP_HIDE => {
            unsafe {
                let _ = KillTimer(hwnd, PULSE_TIMER_ID);
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            0
        }
        WM_DPICHANGED => {
            let dpi = (w_param & 0xFFFF) as u32;
            let geometry = overlay_geometry_for_dpi(screen_width_for_dpi(dpi), dpi);
            unsafe {
                apply_overlay_region(
                    hwnd,
                    geometry.width,
                    geometry.height,
                    scale_i32(BASE_CORNER_RADIUS, dpi),
                );
                let _ = SetWindowPos(
                    hwnd,
                    ptr::null_mut(),
                    geometry.x,
                    geometry.y,
                    geometry.width,
                    geometry.height,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
                let _ = InvalidateRect(hwnd, ptr::null(), 0);
            }
            0
        }
        WM_APP_CLOSE | WM_CLOSE => {
            unsafe {
                let _ = KillTimer(hwnd, PULSE_TIMER_ID);
                let _ = DestroyWindow(hwnd);
            }
            0
        }
        WM_TIMER => {
            if w_param == PULSE_TIMER_ID {
                unsafe {
                    let _ = InvalidateRect(hwnd, ptr::null(), 0);
                }
                0
            } else {
                unsafe { DefWindowProcW(hwnd, message, w_param, l_param) }
            }
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
        let dpi = current_dpi(hwnd);
        let metrics = overlay_metrics(dpi);
        let dot = current_dot_metrics(&metrics);

        if !paint_overlay_antialiased(hdc, &rect, &metrics, &dot) {
            paint_overlay_fallback_gdi(hdc, &rect, &metrics, &dot);
        }

        let _ = SetBkMode(hdc, TRANSPARENT as i32);
        let _ = SetTextColor(hdc, rgb(17, 17, 17) as COLORREF);
        let mut text_rect = RECT {
            left: metrics.text_left,
            top: 0,
            right: rect.right - metrics.text_right,
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

unsafe fn apply_overlay_region(hwnd: HWND, width: i32, height: i32, radius: i32) {
    let square_top = radius.max(1);
    let region = unsafe {
        CreateRoundRectRgn(
            0,
            -square_top,
            width + 1,
            height + 1,
            radius * 2,
            radius * 2,
        )
    };
    if !region.is_null() {
        let _ = unsafe { SetWindowRgn(hwnd, region, 1) };
    }
}

fn paint_overlay_antialiased(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: &RECT,
    metrics: &OverlayMetrics,
    dot_metrics: &DotRenderMetrics,
) -> bool {
    if !ensure_gdiplus() {
        return false;
    }

    unsafe {
        let mut graphics: *mut GpGraphics = ptr::null_mut();
        if GdipCreateFromHDC(hdc, &mut graphics) != 0 || graphics.is_null() {
            return false;
        }

        let mut path: *mut GpPath = ptr::null_mut();
        let mut fill: *mut windows_sys::Win32::Graphics::GdiPlus::GpSolidFill = ptr::null_mut();
        let mut pen: *mut GpPen = ptr::null_mut();
        let mut dot: *mut windows_sys::Win32::Graphics::GdiPlus::GpSolidFill = ptr::null_mut();

        let _ = GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
        let _ = GdipSetCompositingQuality(graphics, CompositingQualityHighQuality);
        let _ = GdipSetPixelOffsetMode(graphics, PixelOffsetModeHalf);
        let _ = GdipSetTextRenderingHint(graphics, TextRenderingHintClearTypeGridFit);
        let _ = GdipGraphicsClear(graphics, argb(255, 247, 247, 247));

        let mut ok = GdipCreatePath(FillModeAlternate, &mut path) == 0 && !path.is_null();
        if ok {
            ok = build_overlay_path(path, rect, metrics.corner_radius);
        }
        if ok {
            ok = GdipCreateSolidFill(argb(255, 247, 247, 247), &mut fill) == 0
                && !fill.is_null()
                && GdipCreatePen1(argb(255, 212, 212, 212), 1.0, UnitPixel, &mut pen) == 0
                && !pen.is_null()
                && GdipCreateSolidFill(dot_metrics.color, &mut dot) == 0
                && !dot.is_null();
        }
        if ok {
            let _ = windows_sys::Win32::Graphics::GdiPlus::GdipSetPenLineJoin(pen, LineJoinRound);
            ok = GdipFillPath(graphics, fill as *mut GpBrush, path) == 0
                && GdipDrawPath(graphics, pen, path) == 0
                && GdipFillEllipseI(
                    graphics,
                    dot as *mut GpBrush,
                    dot_metrics.left,
                    dot_metrics.top,
                    dot_metrics.size,
                    dot_metrics.size,
                ) == 0;
        }

        if !dot.is_null() {
            let _ = GdipDeleteBrush(dot as *mut GpBrush);
        }
        if !pen.is_null() {
            let _ = GdipDeletePen(pen);
        }
        if !fill.is_null() {
            let _ = GdipDeleteBrush(fill as *mut GpBrush);
        }
        if !path.is_null() {
            let _ = GdipDeletePath(path);
        }
        let _ = GdipDeleteGraphics(graphics);

        ok
    }
}

fn paint_overlay_fallback_gdi(
    hdc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: &RECT,
    metrics: &OverlayMetrics,
    dot_metrics: &DotRenderMetrics,
) {
    unsafe {
        let background = CreateSolidBrush(rgb(247, 247, 247));
        let border_pen = windows_sys::Win32::Graphics::Gdi::CreatePen(
            windows_sys::Win32::Graphics::Gdi::PS_SOLID,
            1,
            rgb(212, 212, 212),
        );
        let old_brush = SelectObject(hdc, background as _);
        let old_pen = SelectObject(hdc, border_pen as _);
        let _ = FillRect(hdc, rect, background);
        let _ = RoundRect(
            hdc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            metrics.corner_radius * 2,
            metrics.corner_radius * 2,
        );
        let _ = SelectObject(hdc, old_pen);
        let _ = SelectObject(hdc, old_brush);
        let _ = DeleteObject(border_pen as _);
        let _ = DeleteObject(background as _);

        let dot_brush = CreateSolidBrush(dot_metrics.gdi_color);
        let old_brush = SelectObject(hdc, dot_brush as _);
        let _ = Ellipse(
            hdc,
            dot_metrics.left,
            dot_metrics.top,
            dot_metrics.left + dot_metrics.size,
            dot_metrics.top + dot_metrics.size,
        );
        let _ = SelectObject(hdc, old_brush);
        let _ = DeleteObject(dot_brush as _);
    }
}

unsafe fn build_overlay_path(path: *mut GpPath, rect: &RECT, radius: i32) -> bool {
    let left = rect.left as f32;
    let top = rect.top as f32;
    let right = rect.right as f32 - 1.0;
    let bottom = rect.bottom as f32 - 1.0;
    let radius = radius.max(1) as f32;
    let diameter = radius * 2.0;

    unsafe {
        GdipAddPathLine(path, left, top, right, top) == 0
            && GdipAddPathLine(path, right, top, right, bottom - radius) == 0
            && GdipAddPathArc(
                path,
                right - diameter,
                bottom - diameter,
                diameter,
                diameter,
                0.0,
                90.0,
            ) == 0
            && GdipAddPathLine(path, right - radius, bottom, left + radius, bottom) == 0
            && GdipAddPathArc(
                path,
                left,
                bottom - diameter,
                diameter,
                diameter,
                90.0,
                90.0,
            ) == 0
            && GdipAddPathLine(path, left, bottom - radius, left, top) == 0
            && GdipClosePathFigure(path) == 0
    }
}

fn ensure_gdiplus() -> bool {
    GDIPLUS_TOKEN
        .get_or_init(|| unsafe {
            let mut token = 0usize;
            let input = GdiplusStartupInput {
                GdiplusVersion: 1,
                DebugEventCallback: 0,
                SuppressBackgroundThread: 0,
                SuppressExternalCodecs: 0,
            };

            if GdiplusStartup(&mut token, &input, ptr::null_mut()) == 0 {
                Some(token)
            } else {
                None
            }
        })
        .is_some()
}

#[derive(Debug, Clone, Copy)]
struct OverlayMetrics {
    corner_radius: i32,
    dot_left: i32,
    dot_top: i32,
    dot_size: i32,
    text_left: i32,
    text_right: i32,
}

fn overlay_metrics(dpi: u32) -> OverlayMetrics {
    let height = scale_i32(BASE_OVERLAY_HEIGHT, dpi);
    let dot_size = scale_i32(BASE_DOT_SIZE, dpi);
    OverlayMetrics {
        corner_radius: scale_i32(BASE_CORNER_RADIUS, dpi),
        dot_left: scale_i32(BASE_DOT_LEFT, dpi),
        dot_top: ((height - dot_size) / 2).max(1),
        dot_size,
        text_left: scale_i32(BASE_TEXT_LEFT, dpi),
        text_right: scale_i32(BASE_TEXT_RIGHT, dpi),
    }
}

#[derive(Debug, Clone, Copy)]
struct DotRenderMetrics {
    left: i32,
    top: i32,
    size: i32,
    color: u32,
    gdi_color: COLORREF,
}

fn current_dot_metrics(metrics: &OverlayMetrics) -> DotRenderMetrics {
    let state = OVERLAY_STATE
        .get()
        .and_then(|state| state.lock().ok().map(|value| value.clone()));

    let Some(state) = state else {
        return DotRenderMetrics {
            left: metrics.dot_left,
            top: metrics.dot_top,
            size: metrics.dot_size,
            color: argb(255, 255, 59, 48),
            gdi_color: rgb(255, 59, 48),
        };
    };

    let elapsed = state.pulse_started_at.elapsed().as_secs_f32();
    let wave = (elapsed * std::f32::consts::TAU * 0.75).sin();
    let scale = 1.0 + 0.08 * ((wave + 1.0) * 0.5);
    let size = ((metrics.dot_size as f32) * scale).round() as i32;
    let size = size.max(metrics.dot_size);
    let offset = (size - metrics.dot_size) / 2;
    let tint = ((wave + 1.0) * 0.5 * 12.0).round() as u8;
    let red = 243_u8.saturating_add(tint);
    let green = 58_u8.saturating_add(tint / 6);
    let blue = 48_u8.saturating_add(tint / 8);

    DotRenderMetrics {
        left: metrics.dot_left - offset,
        top: metrics.dot_top - offset,
        size,
        color: argb(255, red, green, blue),
        gdi_color: rgb(red, green, blue),
    }
}

fn overlay_geometry_for_dpi(screen_width: i32, dpi: u32) -> OverlayGeometry {
    compute_top_center_geometry(
        screen_width,
        scale_i32(BASE_OVERLAY_WIDTH, dpi),
        scale_i32(BASE_OVERLAY_HEIGHT, dpi),
        scale_i32(BASE_TOP_MARGIN, dpi),
    )
}

fn current_dpi(hwnd: HWND) -> u32 {
    unsafe {
        let dpi = if hwnd.is_null() {
            GetDpiForSystem()
        } else {
            GetDpiForWindow(hwnd)
        };
        if dpi == 0 { 96 } else { dpi }
    }
}

fn screen_width_for_dpi(dpi: u32) -> i32 {
    let _ = dpi;
    unsafe { GetSystemMetrics(SM_CXSCREEN) }
}

fn scale_i32(value: i32, dpi: u32) -> i32 {
    (((value as i64) * (dpi as i64)) / 96).max(1) as i32
}

const fn argb(alpha: u8, red: u8, green: u8, blue: u8) -> u32 {
    ((alpha as u32) << 24) | ((red as u32) << 16) | ((green as u32) << 8) | blue as u32
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
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) == 0 {
            let _ = SetProcessDPIAware();
        }
    }
}

impl Drop for RecordingOverlay {
    fn drop(&mut self) {
        if Arc::strong_count(&self.hwnd) == 1 {
            self.close();
        }
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
