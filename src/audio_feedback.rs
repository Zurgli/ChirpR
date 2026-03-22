use std::path::Path;

#[derive(Default)]
pub struct AudioFeedback {
    enabled: bool,
    _volume: f32,
}

impl AudioFeedback {
    pub fn new(enabled: bool, volume: f32) -> Self {
        Self {
            enabled,
            _volume: volume,
        }
    }

    pub fn play_start(&self, _path: Option<&Path>) {
        if self.enabled {}
    }

    pub fn play_stop(&self, _path: Option<&Path>) {
        if self.enabled {}
    }

    pub fn play_error(&self, _path: Option<&Path>) {
        if self.enabled {}
    }
}
