use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use arboard::Clipboard;

use crate::keyboard::KeyboardController;
use crate::text_processing::TextProcessor;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClipboardSnapshot {
    Text(String),
    Empty,
    Unavailable,
}

pub struct TextInjector {
    keyboard: Arc<KeyboardController>,
    processor: TextProcessor,
    primary_shortcut: String,
    injection_mode: String,
    paste_mode: String,
    clipboard_behavior: bool,
    clipboard_clear_delay: Duration,
}

impl TextInjector {
    pub fn new(
        keyboard: Arc<KeyboardController>,
        processor: TextProcessor,
        primary_shortcut: &str,
        injection_mode: &str,
        paste_mode: &str,
        clipboard_behavior: bool,
        clipboard_clear_delay_secs: f32,
    ) -> Self {
        Self {
            keyboard,
            processor,
            primary_shortcut: primary_shortcut.to_string(),
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

        // Clear synthetic modifiers first, then wait for the configured
        // recording shortcut itself to be physically released. Without this,
        // fast transcriptions can leak Ctrl/Shift/Space into the first
        // injected characters or accidentally trigger app shortcuts.
        self.keyboard.release_modifiers()?;
        self.keyboard
            .wait_for_shortcut_release(&self.primary_shortcut, Duration::from_millis(750))?;
        thread::sleep(Duration::from_millis(30));

        if cfg!(target_os = "windows") && self.injection_mode == "type" {
            thread::sleep(Duration::from_millis(120));
            self.keyboard.write(&processed)?;
            return Ok(processed);
        }

        let mut clipboard = Clipboard::new().context("failed to open clipboard")?;
        let previous_clipboard = if self.clipboard_behavior {
            snapshot_clipboard_text(&mut clipboard)
        } else {
            ClipboardSnapshot::Unavailable
        };
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
            self.schedule_clipboard_restore(previous_clipboard, processed.clone());
        }

        Ok(processed)
    }

    fn schedule_clipboard_restore(
        &self,
        previous_clipboard: ClipboardSnapshot,
        injected_text: String,
    ) {
        let delay = self.clipboard_clear_delay;
        thread::spawn(move || {
            thread::sleep(delay);
            if let Ok(mut clipboard) = Clipboard::new() {
                let current_text = clipboard.get_text().ok();
                if current_text.as_deref() != Some(injected_text.as_str()) {
                    return;
                }

                match previous_clipboard {
                    // Restore the previous clipboard text when our injected paste
                    // is still the current clipboard contents.
                    ClipboardSnapshot::Text(previous) => {
                        let _ = clipboard.set_text(previous);
                    }
                    ClipboardSnapshot::Empty => {
                        let _ = clipboard.set_text("");
                    }
                    ClipboardSnapshot::Unavailable => {
                        let _ = clipboard.set_text("");
                    }
                }
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

fn snapshot_clipboard_text(clipboard: &mut Clipboard) -> ClipboardSnapshot {
    match clipboard.get_text() {
        Ok(text) if text.is_empty() => ClipboardSnapshot::Empty,
        Ok(text) => ClipboardSnapshot::Text(text),
        Err(_) => ClipboardSnapshot::Unavailable,
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
