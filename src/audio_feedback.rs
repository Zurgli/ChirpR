use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AudioFeedback {
    enabled: bool,
    volume: f32,
    sounds_root: PathBuf,
}

impl AudioFeedback {
    pub fn new(enabled: bool, volume: f32, sounds_root: PathBuf) -> Self {
        Self {
            enabled,
            volume: volume.clamp(0.0, 1.0),
            sounds_root,
        }
    }

    pub fn play_start(&self, override_path: Option<&Path>) {
        self.play_optional_sound("ping-up.wav", override_path);
    }

    pub fn play_stop(&self, override_path: Option<&Path>) {
        self.play_optional_sound("ping-down.wav", override_path);
    }

    pub fn play_error(&self, override_path: Option<&Path>) {
        if !self.enabled {
            return;
        }

        if self.try_play_path(override_path).is_some() {
            return;
        }

        #[cfg(target_os = "windows")]
        unsafe {
            let _ = windows_sys::Win32::System::Diagnostics::Debug::MessageBeep(
                windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONHAND,
            );
        }
    }

    fn play_optional_sound(&self, asset_name: &str, override_path: Option<&Path>) {
        if !self.enabled {
            return;
        }

        if self.try_play_path(override_path).is_some() {
            return;
        }

        let asset_path = self.sounds_root.join(asset_name);
        let _ = self.try_play_path(Some(asset_path.as_path()));
    }

    fn try_play_path(&self, path: Option<&Path>) -> Option<()> {
        let path = path?;
        if !path.is_file() {
            return None;
        }

        #[cfg(target_os = "windows")]
        unsafe {
            use std::os::windows::ffi::OsStrExt;

            let wide_path = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>();

            let _ = self.volume;
            let _ = windows_sys::Win32::Media::Audio::PlaySoundW(
                wide_path.as_ptr(),
                std::ptr::null_mut(),
                windows_sys::Win32::Media::Audio::SND_ASYNC
                    | windows_sys::Win32::Media::Audio::SND_FILENAME
                    | windows_sys::Win32::Media::Audio::SND_NODEFAULT,
            );
            Some(())
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = self.volume;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_stop_methods_are_safe_when_assets_missing() {
        let feedback = AudioFeedback::new(false, 0.25, PathBuf::from("."));
        feedback.play_start(None);
        feedback.play_stop(None);
        feedback.play_error(None);
    }
}
