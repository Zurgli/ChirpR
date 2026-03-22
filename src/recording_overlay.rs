#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayGeometry {
    pub width: i32,
    pub height: i32,
    pub x: i32,
    pub y: i32,
}

#[derive(Default)]
pub struct RecordingOverlay {
    enabled: bool,
    mode: String,
}

impl RecordingOverlay {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            mode: "transcribing".to_string(),
        }
    }

    pub fn show(&mut self, mode: &str) {
        if self.enabled {
            self.mode = mode.to_string();
        }
    }

    pub fn hide(&self) {}

    pub fn close(&self) {}

    pub fn set_mode(&mut self, mode: &str) {
        if self.enabled {
            self.mode = mode.to_string();
        }
    }

    pub fn mode(&self) -> &str {
        &self.mode
    }
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

pub fn enable_dpi_awareness() {}

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
