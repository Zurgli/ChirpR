#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![allow(unsafe_op_in_unsafe_fn)]
#![allow(dead_code)]

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("chirpr-indicator-gallery is only supported on Windows");
}

#[cfg(target_os = "windows")]
mod windows_gallery {
    use std::iter;
    use std::mem::MaybeUninit;
    use std::ptr;
    use std::sync::OnceLock;
    use std::time::Instant;

    use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DT_CENTER, DT_SINGLELINE,
        DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, EndPaint, HDC, InvalidateRect, PAINTSTRUCT,
        SRCCOPY, SelectObject, SetBkMode, SetTextColor, TRANSPARENT, UpdateWindow,
    };
    use windows_sys::Win32::Graphics::GdiPlus::{
        CompositingQualityHighQuality, FillModeAlternate, GdipAddPathArc, GdipAddPathLine,
        GdipClosePathFigure, GdipCreateFromHDC, GdipCreatePath, GdipCreatePen1,
        GdipCreateSolidFill, GdipDeleteBrush, GdipDeleteGraphics, GdipDeletePath, GdipDeletePen,
        GdipDrawArcI, GdipDrawEllipseI, GdipDrawLineI, GdipDrawPath, GdipFillEllipseI,
        GdipFillPath, GdipFillRectangleI, GdipGraphicsClear, GdipSetCompositingQuality,
        GdipSetPenLineJoin, GdipSetPixelOffsetMode, GdipSetSmoothingMode, GdipSetTextRenderingHint,
        GdiplusStartup, GdiplusStartupInput, GpBrush, GpGraphics, GpPath, LineJoinRound,
        PixelOffsetModeHalf, SmoothingModeAntiAlias, TextRenderingHintClearTypeGridFit, UnitPixel,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW,
        GetClientRect, GetMessageW, IDC_ARROW, KillTimer, LoadCursorW, MSG, PostQuitMessage,
        RegisterClassW, SW_SHOW, SetProcessDPIAware, SetTimer, ShowWindow, TranslateMessage,
        WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_SIZE, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
        WS_VISIBLE,
    };

    const WINDOW_CLASS: &str = "ChirpRIndicatorGalleryWindow";
    const WINDOW_TITLE: &str = "ChirpR Indicator Gallery";
    const TIMER_ID: usize = 1;
    const FRAME_MS: u32 = 42;
    const CARD_COLUMNS: i32 = 1;
    const CARD_ROWS: i32 = 1;
    const CARD_COUNT: usize = 1;
    const HEADER_HEIGHT: i32 = 68;
    const OUTER_PAD: i32 = 18;
    const CARD_GAP: i32 = 14;
    const CARD_RADIUS: i32 = 18;
    const CARD_MIN_HEIGHT: i32 = 420;
    const INDICATOR_TOP_PAD: i32 = 40;
    const INDICATOR_HEIGHT: i32 = 230;
    const LABEL_HEIGHT: i32 = 52;

    static START_TIME: OnceLock<Instant> = OnceLock::new();
    static GDIPLUS_TOKEN: OnceLock<Option<usize>> = OnceLock::new();

    #[derive(Clone, Copy)]
    struct IndicatorSpec {
        index: usize,
        name: &'static str,
    }

    const INDICATORS: [IndicatorSpec; CARD_COUNT] = [IndicatorSpec {
        index: 1,
        name: "Single Tapered Sine",
    }];

    #[derive(Clone, Copy)]
    struct RectI {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    impl RectI {
        fn width(self) -> i32 {
            self.right - self.left
        }

        fn height(self) -> i32 {
            self.bottom - self.top
        }

        fn inset(self, dx: i32, dy: i32) -> Self {
            Self {
                left: self.left + dx,
                top: self.top + dy,
                right: self.right - dx,
                bottom: self.bottom - dy,
            }
        }

        fn center_x(self) -> i32 {
            self.left + self.width() / 2
        }

        fn center_y(self) -> i32 {
            self.top + self.height() / 2
        }
    }

    pub fn run() -> anyhow::Result<()> {
        enable_dpi_awareness();
        let _ = START_TIME.set(Instant::now());
        let _ = ensure_gdiplus();

        unsafe {
            let hinstance = GetModuleHandleW(ptr::null());
            let class_name = wide(WINDOW_CLASS);
            let title = wide(WINDOW_TITLE);
            let cursor = LoadCursorW(ptr::null_mut(), IDC_ARROW);

            let wnd = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(window_proc),
                hInstance: hinstance,
                hCursor: cursor,
                lpszClassName: class_name.as_ptr(),
                ..std::mem::zeroed()
            };
            let _ = RegisterClassW(&wnd);

            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1040,
                700,
                ptr::null_mut(),
                ptr::null_mut(),
                hinstance,
                ptr::null(),
            );

            if hwnd.is_null() {
                anyhow::bail!("failed to create gallery window");
            }

            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
            SetTimer(hwnd, TIMER_ID, FRAME_MS, None);

            let mut msg = MaybeUninit::<MSG>::zeroed();
            while GetMessageW(msg.as_mut_ptr(), ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(msg.as_ptr());
                DispatchMessageW(msg.as_ptr());
            }
        }

        Ok(())
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_ERASEBKGND => 1,
            WM_TIMER => {
                if wparam == TIMER_ID {
                    let _ = InvalidateRect(hwnd, ptr::null(), 0);
                    0
                } else {
                    DefWindowProcW(hwnd, message, wparam, lparam)
                }
            }
            WM_SIZE => {
                let _ = InvalidateRect(hwnd, ptr::null(), 0);
                0
            }
            WM_PAINT => {
                paint_gallery(hwnd);
                0
            }
            WM_DESTROY => {
                KillTimer(hwnd, TIMER_ID);
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    fn paint_gallery(hwnd: HWND) {
        unsafe {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if hdc.is_null() {
                return;
            }

            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);
            let width = (rect.right - rect.left).max(1);
            let height = (rect.bottom - rect.top).max(1);

            let mem_dc = CreateCompatibleDC(hdc);
            if mem_dc.is_null() {
                EndPaint(hwnd, &ps);
                return;
            }

            let mem_bitmap = CreateCompatibleBitmap(hdc, width, height);
            if mem_bitmap.is_null() {
                DeleteDC(mem_dc);
                EndPaint(hwnd, &ps);
                return;
            }

            let old_bitmap = SelectObject(mem_dc, mem_bitmap as _);
            render_gallery(mem_dc, width, height);
            let _ = BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);

            SelectObject(mem_dc, old_bitmap);
            DeleteObject(mem_bitmap as _);
            DeleteDC(mem_dc);
            EndPaint(hwnd, &ps);
        }
    }

    fn render_gallery(hdc: HDC, width: i32, height: i32) {
        unsafe {
            let mut graphics: *mut GpGraphics = ptr::null_mut();
            if GdipCreateFromHDC(hdc, &mut graphics) != 0 || graphics.is_null() {
                return;
            }

            let _ = GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
            let _ = GdipSetCompositingQuality(graphics, CompositingQualityHighQuality);
            let _ = GdipSetPixelOffsetMode(graphics, PixelOffsetModeHalf);
            let _ = GdipSetTextRenderingHint(graphics, TextRenderingHintClearTypeGridFit);
            let _ = GdipGraphicsClear(graphics, argb(255, 244, 241, 235));

            let bounds = RectI {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            draw_header(hdc, bounds);

            let elapsed = START_TIME
                .get()
                .map(|start| start.elapsed().as_secs_f32())
                .unwrap_or(0.0);
            let cards_top = HEADER_HEIGHT;
            let available_height =
                (height - cards_top - OUTER_PAD).max(CARD_ROWS * CARD_MIN_HEIGHT);
            let card_width =
                ((width - OUTER_PAD * 2 - CARD_GAP * (CARD_COLUMNS - 1)) / CARD_COLUMNS).max(180);
            let card_height =
                ((available_height - CARD_GAP * (CARD_ROWS - 1)) / CARD_ROWS).max(CARD_MIN_HEIGHT);

            for (slot, indicator) in INDICATORS.iter().enumerate() {
                let col = (slot as i32) % CARD_COLUMNS;
                let row = (slot as i32) / CARD_COLUMNS;
                let left = OUTER_PAD + col * (card_width + CARD_GAP);
                let top = cards_top + row * (card_height + CARD_GAP);
                let card = RectI {
                    left,
                    top,
                    right: left + card_width,
                    bottom: top + card_height,
                };
                draw_card(graphics, hdc, card, *indicator, elapsed);
            }

            let _ = GdipDeleteGraphics(graphics);
        }
    }

    fn draw_header(hdc: HDC, bounds: RectI) {
        unsafe {
            let _ = SetBkMode(hdc, TRANSPARENT as i32);
            let _ = SetTextColor(hdc, rgb(45, 42, 38) as COLORREF);

            let mut title_rect = RECT {
                left: OUTER_PAD,
                top: 14,
                right: bounds.right - OUTER_PAD,
                bottom: 42,
            };
            let mut subtitle_rect = RECT {
                left: OUTER_PAD,
                top: 40,
                right: bounds.right - OUTER_PAD,
                bottom: HEADER_HEIGHT - 8,
            };

            let title = wide("ChirpR Wave Preview");
            let subtitle = wide(
                "Step 1: one vertically centered sine wave, tapered to zero at the edges, traveling right-to-left.",
            );

            let _ = DrawTextW(
                hdc,
                title.as_ptr(),
                -1,
                &mut title_rect,
                DT_SINGLELINE | DT_VCENTER,
            );
            let _ = SetTextColor(hdc, rgb(92, 86, 79) as COLORREF);
            let _ = DrawTextW(
                hdc,
                subtitle.as_ptr(),
                -1,
                &mut subtitle_rect,
                DT_SINGLELINE | DT_VCENTER,
            );
        }
    }

    fn draw_card(
        graphics: *mut GpGraphics,
        hdc: HDC,
        card: RectI,
        indicator: IndicatorSpec,
        t: f32,
    ) {
        unsafe {
            let mut path: *mut GpPath = ptr::null_mut();
            let mut fill = ptr::null_mut();
            let mut border = ptr::null_mut();
            let _ = GdipCreatePath(FillModeAlternate, &mut path);
            let _ = build_round_rect_path(path, card, CARD_RADIUS);
            let _ = GdipCreateSolidFill(argb(255, 255, 252, 248), &mut fill);
            let _ = GdipCreatePen1(argb(255, 214, 207, 198), 1.0, UnitPixel, &mut border);
            let _ = GdipSetPenLineJoin(border, LineJoinRound);
            let _ = GdipFillPath(graphics, fill as *mut GpBrush, path);
            let _ = GdipDrawPath(graphics, border, path);
            let _ = GdipDeleteBrush(fill as *mut GpBrush);
            let _ = GdipDeletePen(border);
            let _ = GdipDeletePath(path);
        }

        let indicator_rect = RectI {
            left: card.left + 14,
            top: card.top + INDICATOR_TOP_PAD,
            right: card.right - 14,
            bottom: card.top + INDICATOR_TOP_PAD + INDICATOR_HEIGHT,
        };
        draw_indicator(graphics, indicator.index, indicator_rect, t);

        unsafe {
            let _ = SetBkMode(hdc, TRANSPARENT as i32);
            let _ = SetTextColor(hdc, rgb(44, 42, 39) as COLORREF);

            let mut number_rect = RECT {
                left: card.left + 14,
                top: indicator_rect.bottom + 14,
                right: card.right - 14,
                bottom: indicator_rect.bottom + 14 + LABEL_HEIGHT,
            };
            let mut name_rect = RECT {
                left: card.left + 14,
                top: number_rect.bottom - 6,
                right: card.right - 14,
                bottom: card.bottom - 12,
            };

            let number = wide(&format!("{:02}", indicator.index));
            let name = wide(indicator.name);
            let _ = DrawTextW(
                hdc,
                number.as_ptr(),
                -1,
                &mut number_rect,
                DT_CENTER | DT_SINGLELINE | DT_VCENTER,
            );
            let _ = SetTextColor(hdc, rgb(111, 101, 91) as COLORREF);
            let _ = DrawTextW(
                hdc,
                name.as_ptr(),
                -1,
                &mut name_rect,
                DT_CENTER | DT_SINGLELINE | DT_VCENTER,
            );
        }
    }

    fn draw_indicator(graphics: *mut GpGraphics, variant: usize, rect: RectI, t: f32) {
        match variant {
            1 => draw_step_one_sine(graphics, rect, t),
            _ => draw_step_one_sine(graphics, rect, t),
        }
    }

    fn draw_step_one_sine(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let preview = rect.inset(18, 22);
        draw_sine_curve(
            graphics,
            preview,
            t,
            2.1,
            preview.height() as f32 * 0.34,
            0.68,
            1.2,
            0.0,
            argb(240, 228, 82, 63),
            3.0,
        );
        draw_sine_curve(
            graphics,
            preview,
            t,
            2.1,
            preview.height() as f32 * 0.34,
            0.68,
            1.2,
            std::f32::consts::TAU / 3.0,
            argb(170, 110, 168, 245),
            3.0,
        );
    }

    fn draw_halo_soft(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let phase = pulse_phase(t, 0.48, 0.0);
        let ring = 18 + (phase * 12.0).round() as i32;
        let alpha = (16.0 + (1.0 - phase) * 72.0).round() as u8;
        stroke_circle(
            graphics,
            center_circle(rect, ring),
            argb(alpha, 236, 84, 67),
            2.0,
        );
        fill_circle(graphics, center_circle(rect, 10), argb(220, 236, 84, 67));
    }

    fn draw_halo_tight(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let phase = pulse_phase(t, 0.74, 0.4);
        let ring = 14 + (phase * 8.0).round() as i32;
        let alpha = (28.0 + (1.0 - phase) * 120.0).round() as u8;
        stroke_circle(
            graphics,
            center_circle(rect, ring),
            argb(alpha, 236, 84, 67),
            2.6,
        );
        fill_circle(graphics, center_circle(rect, 9), argb(235, 236, 84, 67));
    }

    fn draw_halo_dual(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let outer = pulse_phase(t, 0.55, 0.0);
        let inner = pulse_phase(t, 0.55, 1.7);
        stroke_circle(
            graphics,
            center_circle(rect, 20 + (outer * 12.0).round() as i32),
            argb((18.0 + (1.0 - outer) * 90.0).round() as u8, 236, 84, 67),
            2.0,
        );
        stroke_circle(
            graphics,
            center_circle(rect, 14 + (inner * 8.0).round() as i32),
            argb((24.0 + (1.0 - inner) * 130.0).round() as u8, 244, 118, 78),
            2.0,
        );
        fill_circle(graphics, center_circle(rect, 8), argb(228, 236, 84, 67));
    }

    fn draw_halo_beacon(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let phase = pulse_phase(t, 0.42, 0.0);
        for idx in 0..3 {
            let local = (phase + idx as f32 * 0.22).fract();
            let ring = 12 + (local * 22.0).round() as i32;
            let alpha = ((1.0 - local) * 95.0).round() as u8;
            stroke_circle(
                graphics,
                center_circle(rect, ring),
                argb(alpha, 236, 84, 67),
                1.8,
            );
        }
        fill_circle(graphics, center_circle(rect, 8), argb(230, 236, 84, 67));
    }

    fn draw_halo_orbit(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let base = center_circle(rect, 28);
        stroke_circle(graphics, base, argb(52, 220, 188, 181), 1.6);
        let pulse = pulse_phase(t, 0.6, 0.0);
        stroke_circle(
            graphics,
            center_circle(rect, 16 + (pulse * 8.0).round() as i32),
            argb((22.0 + (1.0 - pulse) * 105.0).round() as u8, 236, 84, 67),
            2.0,
        );
        let angle = t * std::f32::consts::TAU * 0.42;
        let x = rect.center_x() as f32 + 14.0 * angle.cos();
        let y = rect.center_y() as f32 + 14.0 * angle.sin();
        fill_circle(graphics, center_circle(rect, 7), argb(210, 236, 84, 67));
        fill_circle(
            graphics,
            RectI {
                left: x.round() as i32 - 3,
                top: y.round() as i32 - 3,
                right: x.round() as i32 + 3,
                bottom: y.round() as i32 + 3,
            },
            argb(245, 249, 126, 84),
        );
    }

    fn draw_halo_bloom(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let phase = pulse_phase(t, 0.52, 0.2);
        let glow = 18 + (phase * 16.0).round() as i32;
        fill_circle(
            graphics,
            center_circle(rect, glow),
            argb((20.0 + phase * 52.0).round() as u8, 246, 131, 88),
        );
        fill_circle(
            graphics,
            center_circle(rect, 18 + (phase * 8.0).round() as i32),
            argb((28.0 + phase * 70.0).round() as u8, 240, 100, 74),
        );
        fill_circle(graphics, center_circle(rect, 10), argb(240, 236, 84, 67));
    }

    fn draw_sine_classic(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        draw_sine_curve(
            graphics,
            rect,
            t,
            2.2,
            10.0,
            0.42,
            1.2,
            0.0,
            argb(235, 219, 80, 63),
            2.0,
        );
    }

    fn draw_sine_fast(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        draw_sine_curve(
            graphics,
            rect,
            t,
            3.0,
            11.0,
            0.54,
            1.5,
            0.0,
            argb(240, 229, 86, 64),
            2.0,
        );
    }

    fn draw_sine_double(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        draw_sine_curve(
            graphics,
            rect,
            t,
            2.1,
            8.0,
            0.38,
            1.8,
            0.0,
            argb(210, 224, 82, 63),
            1.8,
        );
        draw_sine_curve(
            graphics,
            rect,
            t,
            2.1,
            -8.0,
            0.38,
            1.8,
            0.0,
            argb(135, 224, 82, 63),
            1.8,
        );
    }

    fn draw_sine_dots(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        for step in 0..18 {
            let progress = step as f32 / 17.0;
            let x = rect.left + 8 + (progress * (rect.width() - 16) as f32).round() as i32;
            let envelope = (progress * std::f32::consts::PI).sin().max(0.0).powf(1.6);
            let y =
                rect.center_y() as f32 + 10.0 * envelope * ((t * 2.7 + step as f32 * 0.55).sin());
            let phase = pulse_phase(t, 0.9, step as f32 * 0.25);
            let size = 3 + (phase * 3.0).round() as i32;
            fill_circle(
                graphics,
                RectI {
                    left: x - size,
                    top: y.round() as i32 - size,
                    right: x + size,
                    bottom: y.round() as i32 + size,
                },
                argb((100.0 + phase * 140.0).round() as u8, 227, 84, 64),
            );
        }
    }

    fn draw_sine_ribbon(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        draw_sine_curve(
            graphics,
            rect,
            t,
            2.0,
            9.0,
            0.36,
            1.35,
            0.0,
            argb(120, 233, 111, 83),
            4.0,
        );
        draw_sine_curve(
            graphics,
            rect,
            t,
            2.0,
            9.0,
            0.36,
            1.35,
            0.0,
            argb(245, 224, 82, 63),
            2.0,
        );
    }

    fn draw_sine_sweep(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        draw_sine_curve(
            graphics,
            rect,
            t,
            2.25,
            10.0,
            0.44,
            1.6,
            0.0,
            argb(220, 220, 80, 63),
            2.0,
        );
        let sweep = pulse_phase(t, 0.68, -0.8);
        let x = rect.left + 8 + (sweep * (rect.width() - 16) as f32).round() as i32;
        fill_circle(
            graphics,
            RectI {
                left: x - 5,
                top: rect.center_y() - 5,
                right: x + 5,
                bottom: rect.center_y() + 5,
            },
            argb(240, 245, 137, 88),
        );
    }

    fn draw_sine_curve(
        graphics: *mut GpGraphics,
        rect: RectI,
        t: f32,
        speed: f32,
        amplitude: f32,
        wavelength: f32,
        taper_power: f32,
        phase_offset: f32,
        color: u32,
        width: f32,
    ) {
        let mut last: Option<(i32, i32)> = None;
        for step in 0..28 {
            let progress = step as f32 / 27.0;
            let x = rect.left + 6 + (progress * (rect.width() - 12) as f32).round() as i32;
            let envelope = (progress * std::f32::consts::PI)
                .sin()
                .max(0.0)
                .powf(taper_power);
            let y = rect.center_y() as f32
                + amplitude
                    * envelope
                    * ((t * speed + step as f32 * wavelength + phase_offset).sin());
            let point = (x, y.round() as i32);
            if let Some(prev) = last {
                draw_line(graphics, prev.0, prev.1, point.0, point.1, color, width);
            }
            last = Some(point);
        }
    }

    fn pulse_phase(t: f32, hz: f32, offset: f32) -> f32 {
        ((t * std::f32::consts::TAU * hz + offset).sin() + 1.0) * 0.5
    }

    fn center_circle(rect: RectI, size: i32) -> RectI {
        let cx = rect.center_x();
        let cy = rect.center_y();
        RectI {
            left: cx - size / 2,
            top: cy - size / 2,
            right: cx + size / 2,
            bottom: cy + size / 2,
        }
    }

    fn draw_breathing_dot(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let phase = pulse_phase(t, 0.55, 0.0);
        let alpha = (45.0 + phase * 210.0).round() as u8;
        fill_circle(graphics, center_circle(rect, 14), argb(alpha, 242, 76, 61));
    }

    fn draw_halo_pulse(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let phase = pulse_phase(t, 0.5, 0.0);
        let ring = 16 + (phase * 14.0).round() as i32;
        let alpha = (18.0 + (1.0 - phase) * 80.0).round() as u8;
        stroke_circle(
            graphics,
            center_circle(rect, ring),
            argb(alpha, 236, 84, 67),
            2.0,
        );
        fill_circle(graphics, center_circle(rect, 10), argb(220, 236, 84, 67));
    }

    fn draw_twin_dots(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let spacing = 18;
        let left = RectI {
            left: rect.center_x() - spacing - 5,
            top: rect.center_y() - 5,
            right: rect.center_x() - spacing + 5,
            bottom: rect.center_y() + 5,
        };
        let right = RectI {
            left: rect.center_x() + spacing - 5,
            top: rect.center_y() - 5,
            right: rect.center_x() + spacing + 5,
            bottom: rect.center_y() + 5,
        };
        fill_circle(
            graphics,
            left,
            argb((60.0 + pulse_phase(t, 0.7, 0.0) * 190.0) as u8, 231, 76, 63),
        );
        fill_circle(
            graphics,
            right,
            argb((60.0 + pulse_phase(t, 0.7, 2.2) * 190.0) as u8, 231, 76, 63),
        );
    }

    fn draw_three_dots(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        for idx in 0..3 {
            let phase = pulse_phase(t, 0.9, idx as f32 * 0.9);
            let x = rect.center_x() - 16 + idx * 16;
            let size = 6 + (phase * 4.0).round() as i32;
            fill_circle(
                graphics,
                RectI {
                    left: x - size / 2,
                    top: rect.center_y() - size / 2,
                    right: x + size / 2,
                    bottom: rect.center_y() + size / 2,
                },
                argb((80.0 + phase * 175.0) as u8, 232, 78, 61),
            );
        }
    }

    fn draw_wave_bars(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        for idx in 0..5 {
            let phase = pulse_phase(t, 0.8, idx as f32 * 0.7);
            let h = 10 + (phase * 22.0).round() as i32;
            let x = rect.center_x() - 24 + idx * 12;
            fill_rect(
                graphics,
                RectI {
                    left: x,
                    top: rect.center_y() - h / 2,
                    right: x + 7,
                    bottom: rect.center_y() + h / 2,
                },
                argb(230, 222, 79, 60),
            );
        }
    }

    fn draw_equalizer(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        for idx in 0..4 {
            let phase = pulse_phase(t, 1.1, idx as f32 * 1.4);
            let h = 8 + (phase * 26.0).round() as i32;
            let x = rect.center_x() - 24 + idx * 14;
            fill_rect(
                graphics,
                RectI {
                    left: x,
                    top: rect.center_y() - h / 2,
                    right: x + 10,
                    bottom: rect.center_y() + h / 2,
                },
                argb(220, 198, 88, 64),
            );
        }
    }

    fn draw_spinner(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let radius = 16.0_f32;
        let active = ((t * 8.0) as i32).rem_euclid(8);
        for idx in 0..8 {
            let angle = idx as f32 / 8.0 * std::f32::consts::TAU;
            let x = rect.center_x() as f32 + radius * angle.cos();
            let y = rect.center_y() as f32 + radius * angle.sin();
            let alpha = if idx == active {
                255
            } else {
                70 + (((idx + 8 - active) % 8) * 20) as u8
            };
            fill_circle(
                graphics,
                RectI {
                    left: x.round() as i32 - 3,
                    top: y.round() as i32 - 3,
                    right: x.round() as i32 + 3,
                    bottom: y.round() as i32 + 3,
                },
                argb(alpha, 229, 77, 61),
            );
        }
    }

    fn draw_orbit(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        stroke_circle(
            graphics,
            center_circle(rect, 28),
            argb(80, 221, 160, 150),
            1.5,
        );
        let angle = t * std::f32::consts::TAU * 0.55;
        let x = rect.center_x() as f32 + 14.0 * angle.cos();
        let y = rect.center_y() as f32 + 14.0 * angle.sin();
        fill_circle(graphics, center_circle(rect, 6), argb(120, 234, 89, 69));
        fill_circle(
            graphics,
            RectI {
                left: x as i32 - 5,
                top: y as i32 - 5,
                right: x as i32 + 5,
                bottom: y as i32 + 5,
            },
            argb(255, 234, 89, 69),
        );
    }

    fn draw_heartbeat(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let baseline = rect.center_y();
        let shift = (((t * 50.0) as i32) % 24) - 12;
        let points = [
            (rect.left + 4, baseline),
            (rect.left + 16 + shift, baseline),
            (rect.left + 22 + shift, baseline - 10),
            (rect.left + 28 + shift, baseline + 10),
            (rect.left + 34 + shift, baseline - 16),
            (rect.left + 40 + shift, baseline + 2),
            (rect.left + 50 + shift, baseline),
            (rect.right - 4, baseline),
        ];
        for pair in points.windows(2) {
            draw_line(
                graphics,
                pair[0].0,
                pair[0].1,
                pair[1].0,
                pair[1].1,
                argb(245, 226, 78, 60),
                2.0,
            );
        }
    }

    fn draw_scanner(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let lane = rect.inset(8, 12);
        fill_rect(graphics, lane, argb(30, 217, 205, 199));
        let phase = pulse_phase(t, 0.65, -1.2);
        let x = lane.left + (phase * (lane.width() - 10) as f32).round() as i32;
        fill_rect(
            graphics,
            RectI {
                left: x,
                top: lane.top,
                right: x + 10,
                bottom: lane.bottom,
            },
            argb(190, 233, 87, 63),
        );
    }

    fn draw_ripple(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let phase = pulse_phase(t, 0.45, 0.0);
        let ring = 10 + (phase * 22.0).round() as i32;
        let alpha = (20.0 + (1.0 - phase) * 140.0).round() as u8;
        stroke_circle(
            graphics,
            center_circle(rect, ring),
            argb(alpha, 225, 81, 63),
            2.0,
        );
        fill_circle(graphics, center_circle(rect, 8), argb(220, 225, 81, 63));
    }

    fn draw_sine_wave(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let amp = 10.0;
        let mut last: Option<(i32, i32)> = None;
        for step in 0..26 {
            let x = rect.left + 6 + step * ((rect.width() - 12) / 25);
            let y = rect.center_y() as f32 + amp * ((t * 2.2 + step as f32 * 0.4).sin());
            let point = (x, y.round() as i32);
            if let Some(prev) = last {
                draw_line(
                    graphics,
                    prev.0,
                    prev.1,
                    point.0,
                    point.1,
                    argb(235, 219, 80, 63),
                    2.0,
                );
            }
            last = Some(point);
        }
    }

    fn draw_twin_rings(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let left = RectI {
            left: rect.center_x() - 24,
            top: rect.center_y() - 12,
            right: rect.center_x(),
            bottom: rect.center_y() + 12,
        };
        let right = RectI {
            left: rect.center_x(),
            top: rect.center_y() - 12,
            right: rect.center_x() + 24,
            bottom: rect.center_y() + 12,
        };
        stroke_circle(
            graphics,
            left,
            argb(
                (80.0 + pulse_phase(t, 0.55, 0.0) * 170.0) as u8,
                228,
                82,
                63,
            ),
            2.0,
        );
        stroke_circle(
            graphics,
            right,
            argb(
                (80.0 + pulse_phase(t, 0.55, 1.6) * 170.0) as u8,
                228,
                82,
                63,
            ),
            2.0,
        );
    }

    fn draw_arc_sweep(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let bounds = center_circle(rect, 28);
        stroke_arc(graphics, bounds, 0.0, 360.0, argb(40, 200, 190, 184), 2.0);
        let start = (t * 210.0) % 360.0;
        stroke_arc(graphics, bounds, start, 105.0, argb(250, 231, 82, 62), 3.0);
    }

    fn draw_bounce(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let phase = pulse_phase(t, 0.9, 0.0);
        let y = rect.bottom - 10 - (phase * 20.0).round() as i32;
        fill_rect(
            graphics,
            RectI {
                left: rect.center_x() - 12,
                top: rect.bottom - 8,
                right: rect.center_x() + 12,
                bottom: rect.bottom - 4,
            },
            argb(28, 40, 40, 40),
        );
        fill_circle(
            graphics,
            RectI {
                left: rect.center_x() - 5,
                top: y - 5,
                right: rect.center_x() + 5,
                bottom: y + 5,
            },
            argb(240, 233, 81, 61),
        );
    }

    fn draw_comet(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let phase = pulse_phase(t, 0.55, -1.0);
        let x = rect.left + 8 + (phase * (rect.width() - 20) as f32).round() as i32;
        for idx in 0..5 {
            let alpha = 220_i32.saturating_sub(idx * 40) as u8;
            let px = x - idx * 8;
            fill_circle(
                graphics,
                RectI {
                    left: px - 4,
                    top: rect.center_y() - 4,
                    right: px + 4,
                    bottom: rect.center_y() + 4,
                },
                argb(alpha, 236, 86, 63),
            );
        }
    }

    fn draw_chevrons(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let active = ((t * 4.5) as i32).rem_euclid(3);
        for idx in 0..3 {
            let alpha = if idx == active { 255 } else { 85 };
            let x = rect.center_x() - 18 + idx * 14;
            draw_line(
                graphics,
                x - 4,
                rect.center_y() - 8,
                x + 4,
                rect.center_y(),
                argb(alpha, 224, 82, 62),
                2.0,
            );
            draw_line(
                graphics,
                x - 4,
                rect.center_y() + 8,
                x + 4,
                rect.center_y(),
                argb(alpha, 224, 82, 62),
                2.0,
            );
        }
    }

    fn draw_capsules(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        for idx in 0..4 {
            let phase = pulse_phase(t, 0.8, idx as f32 * 1.1);
            let w = 8 + (phase * 9.0).round() as i32;
            let x = rect.center_x() - 26 + idx * 16;
            fill_rect(
                graphics,
                RectI {
                    left: x,
                    top: rect.center_y() - 5,
                    right: x + w,
                    bottom: rect.center_y() + 5,
                },
                argb((90.0 + phase * 150.0) as u8, 231, 84, 62),
            );
        }
    }

    fn draw_radar(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let bounds = center_circle(rect, 30);
        stroke_circle(graphics, bounds, argb(65, 203, 197, 190), 1.5);
        stroke_circle(
            graphics,
            center_circle(rect, 18),
            argb(45, 203, 197, 190),
            1.0,
        );
        let angle = t * 140.0;
        let rad = angle.to_radians();
        let x = rect.center_x() + (14.0 * rad.cos()).round() as i32;
        let y = rect.center_y() + (14.0 * rad.sin()).round() as i32;
        draw_line(
            graphics,
            rect.center_x(),
            rect.center_y(),
            x,
            y,
            argb(170, 229, 86, 64),
            2.0,
        );
        fill_circle(
            graphics,
            RectI {
                left: x - 3,
                top: y - 3,
                right: x + 3,
                bottom: y + 3,
            },
            argb(255, 229, 86, 64),
        );
    }

    fn draw_beacon(graphics: *mut GpGraphics, rect: RectI, t: f32) {
        let phase = pulse_phase(t, 0.75, 0.0);
        let alpha = (60.0 + phase * 180.0).round() as u8;
        stroke_circle(
            graphics,
            center_circle(rect, 20),
            argb(alpha / 2, 235, 86, 63),
            2.0,
        );
        stroke_circle(
            graphics,
            center_circle(rect, 30),
            argb(alpha / 3, 235, 86, 63),
            2.0,
        );
        fill_circle(graphics, center_circle(rect, 8), argb(alpha, 235, 86, 63));
    }

    fn fill_rect(graphics: *mut GpGraphics, rect: RectI, color: u32) {
        unsafe {
            let mut brush = ptr::null_mut();
            let _ = GdipCreateSolidFill(color, &mut brush);
            let _ = GdipFillRectangleI(
                graphics,
                brush as *mut GpBrush,
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
            );
            let _ = GdipDeleteBrush(brush as *mut GpBrush);
        }
    }

    fn fill_circle(graphics: *mut GpGraphics, rect: RectI, color: u32) {
        unsafe {
            let mut brush = ptr::null_mut();
            let _ = GdipCreateSolidFill(color, &mut brush);
            let _ = GdipFillEllipseI(
                graphics,
                brush as *mut GpBrush,
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
            );
            let _ = GdipDeleteBrush(brush as *mut GpBrush);
        }
    }

    fn stroke_circle(graphics: *mut GpGraphics, rect: RectI, color: u32, width: f32) {
        unsafe {
            let mut pen = ptr::null_mut();
            let _ = GdipCreatePen1(color, width, UnitPixel, &mut pen);
            let _ = GdipSetPenLineJoin(pen, LineJoinRound);
            let _ = GdipDrawEllipseI(
                graphics,
                pen,
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
            );
            let _ = GdipDeletePen(pen);
        }
    }

    fn stroke_arc(
        graphics: *mut GpGraphics,
        rect: RectI,
        start: f32,
        sweep: f32,
        color: u32,
        width: f32,
    ) {
        unsafe {
            let mut pen = ptr::null_mut();
            let _ = GdipCreatePen1(color, width, UnitPixel, &mut pen);
            let _ = GdipSetPenLineJoin(pen, LineJoinRound);
            let _ = GdipDrawArcI(
                graphics,
                pen,
                rect.left,
                rect.top,
                rect.width(),
                rect.height(),
                start,
                sweep,
            );
            let _ = GdipDeletePen(pen);
        }
    }

    fn draw_line(
        graphics: *mut GpGraphics,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: u32,
        width: f32,
    ) {
        unsafe {
            let mut pen = ptr::null_mut();
            let _ = GdipCreatePen1(color, width, UnitPixel, &mut pen);
            let _ = GdipSetPenLineJoin(pen, LineJoinRound);
            let _ = GdipDrawLineI(graphics, pen, x1, y1, x2, y2);
            let _ = GdipDeletePen(pen);
        }
    }

    unsafe fn build_round_rect_path(path: *mut GpPath, rect: RectI, radius: i32) -> bool {
        let left = rect.left as f32;
        let top = rect.top as f32;
        let right = rect.right as f32 - 1.0;
        let bottom = rect.bottom as f32 - 1.0;
        let radius = radius.max(1) as f32;
        let diameter = radius * 2.0;

        GdipAddPathLine(path, left + radius, top, right - radius, top) == 0
            && GdipAddPathArc(path, right - diameter, top, diameter, diameter, 270.0, 90.0) == 0
            && GdipAddPathLine(path, right, top + radius, right, bottom - radius) == 0
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
            && GdipAddPathLine(path, left, bottom - radius, left, top + radius) == 0
            && GdipAddPathArc(path, left, top, diameter, diameter, 180.0, 90.0) == 0
            && GdipClosePathFigure(path) == 0
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

    fn enable_dpi_awareness() {
        unsafe {
            if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) == 0 {
                let _ = SetProcessDPIAware();
            }
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(iter::once(0)).collect()
    }

    const fn argb(alpha: u8, red: u8, green: u8, blue: u8) -> u32 {
        ((alpha as u32) << 24) | ((red as u32) << 16) | ((green as u32) << 8) | blue as u32
    }

    const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
        red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
    }
}

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = windows_gallery::run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
