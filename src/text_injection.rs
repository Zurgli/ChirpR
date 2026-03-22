use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use arboard::Clipboard;

use crate::keyboard::KeyboardController;
use crate::text_processing::TextProcessor;

pub struct TextInjector {
    keyboard: Arc<KeyboardController>,
    processor: TextProcessor,
    injection_mode: String,
    paste_mode: String,
    clipboard_behavior: bool,
    clipboard_clear_delay: Duration,
}

impl TextInjector {
    pub fn new(
        keyboard: Arc<KeyboardController>,
        processor: TextProcessor,
        injection_mode: &str,
        paste_mode: &str,
        clipboard_behavior: bool,
        clipboard_clear_delay_secs: f32,
    ) -> Self {
        Self {
            keyboard,
            processor,
            injection_mode: injection_mode.to_string(),
            paste_mode: paste_mode.to_string(),
            clipboard_behavior,
            clipboard_clear_delay: Duration::from_secs_f32(clipboard_clear_delay_secs.max(0.1)),
        }
    }

    pub fn process(&self, text: &str) -> String {
        self.processor.process(text)
    }

    pub fn inject(&self, text: &str) -> Result<String> {
        let processed = self.process(text);
        if processed.is_empty() {
            return Ok(processed);
        }

        if cfg!(target_os = "windows") && self.injection_mode == "type" {
            thread::sleep(Duration::from_millis(120));
            self.keyboard.write(&processed)?;
            return Ok(processed);
        }

        let mut clipboard = Clipboard::new().context("failed to open clipboard")?;
        clipboard
            .set_text(processed.clone())
            .context("failed to write text to clipboard")?;
        thread::sleep(Duration::from_millis(120));

        let combo = if self.paste_mode == "ctrl" {
            "ctrl+v"
        } else {
            "ctrl+shift+v"
        };
        self.keyboard.send(combo)?;

        if self.clipboard_behavior {
            self.schedule_clipboard_clear();
        }

        Ok(processed)
    }

    fn schedule_clipboard_clear(&self) {
        let delay = self.clipboard_clear_delay;
        thread::spawn(move || {
            thread::sleep(delay);
            if let Ok(mut clipboard) = Clipboard::new() {
                let _ = clipboard.set_text("");
            }
        });
    }
    pub fn from_processing(
        word_overrides: std::collections::BTreeMap<String, String>,
        post_processing: &str,
    ) -> TextProcessor {
        TextProcessor::new(word_overrides, post_processing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn process_applies_overrides_and_styles() {
        let processor = TextInjector::from_processing(
            BTreeMap::from([("parra keat".into(), "parakeet".into())]),
            "sentence case\nappend: done",
        );

        assert_eq!(
            processor.process("parra keat is HERE"),
            "Parakeet is here done"
        );
    }
}
