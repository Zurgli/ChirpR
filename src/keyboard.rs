use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use rdev::{Event, EventType};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ShortcutKey {
    Control,
    Shift,
    Alt,
    Meta,
    Named(rdev::Key),
}

#[derive(Debug, Clone)]
struct ListenerState {
    pressed: HashSet<rdev::Key>,
    triggered: bool,
}

pub struct KeyboardController {
    enigo: Mutex<Enigo>,
}

pub struct KeyboardShortcutListener {
    trigger_rx: Receiver<()>,
}

impl KeyboardController {
    pub fn new() -> Result<Self> {
        let enigo =
            Enigo::new(&Settings::default()).context("failed to initialize keyboard automation")?;
        Ok(Self {
            enigo: Mutex::new(enigo),
        })
    }

    pub fn send(&self, combination: &str) -> Result<()> {
        let mut enigo = self
            .enigo
            .lock()
            .map_err(|_| anyhow::anyhow!("keyboard automation lock poisoned"))?;
        let parts = parse_send_combination(combination)?;

        for part in &parts {
            enigo
                .key(*part, Direction::Press)
                .context("failed to press keyboard combination")?;
        }
        for part in parts.iter().rev() {
            enigo
                .key(*part, Direction::Release)
                .context("failed to release keyboard combination")?;
        }

        Ok(())
    }

    pub fn write(&self, text: &str) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            return send_unicode_text(text);
        }

        #[cfg(not(target_os = "windows"))]
        {
            let mut enigo = self
                .enigo
                .lock()
                .map_err(|_| anyhow::anyhow!("keyboard automation lock poisoned"))?;
            enigo
                .text(text)
                .context("failed to type text into active window")
        }
    }

    pub fn release_modifiers(&self) -> Result<()> {
        let mut enigo = self
            .enigo
            .lock()
            .map_err(|_| anyhow::anyhow!("keyboard automation lock poisoned"))?;

        for key in [Key::Shift, Key::Control, Key::Alt, Key::Meta] {
            enigo
                .key(key, Direction::Release)
                .context("failed to release keyboard modifier")?;
        }

        Ok(())
    }

    pub fn wait_for_shortcut_release(&self, shortcut: &str, timeout: Duration) -> Result<()> {
        let required_keys = parse_shortcut(shortcut)?;

        #[cfg(target_os = "windows")]
        {
            return wait_for_windows_shortcut_release(&required_keys, timeout);
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = required_keys;
            let _ = timeout;
            Ok(())
        }
    }
}

#[cfg(target_os = "windows")]
fn send_unicode_text(text: &str) -> Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput,
    };

    // SendInput with KEYEVENTF_UNICODE is the native Windows path for text input.
    // It generates VK_PACKET/WM_CHAR text rather than trying to map characters
    // through the active keyboard layout.
    for unit in text.encode_utf16() {
        let key_down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: unit,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let key_up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: unit,
                    dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [key_down, key_up];
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent != inputs.len() as u32 {
            bail!("failed to inject unicode text with SendInput");
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn wait_for_windows_shortcut_release(
    required_keys: &[ShortcutKey],
    timeout: Duration,
) -> Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_A, VK_B, VK_BACK, VK_C, VK_CONTROL, VK_D, VK_DELETE, VK_DOWN, VK_E,
        VK_ESCAPE, VK_F, VK_G, VK_H, VK_I, VK_J, VK_K, VK_L, VK_LCONTROL, VK_LEFT, VK_LMENU,
        VK_LSHIFT, VK_LWIN, VK_M, VK_MENU, VK_N, VK_O, VK_P, VK_Q, VK_R, VK_RCONTROL, VK_RETURN,
        VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_S, VK_SHIFT, VK_SPACE, VK_T, VK_TAB, VK_U,
        VK_UP, VK_V, VK_W, VK_X, VK_Y, VK_Z,
    };

    let virtual_keys = required_keys
        .iter()
        .flat_map(|key| match key {
            ShortcutKey::Control => vec![VK_LCONTROL, VK_RCONTROL, VK_CONTROL],
            ShortcutKey::Shift => vec![VK_LSHIFT, VK_RSHIFT, VK_SHIFT],
            ShortcutKey::Alt => vec![VK_LMENU, VK_RMENU, VK_MENU],
            ShortcutKey::Meta => vec![VK_LWIN, VK_RWIN],
            ShortcutKey::Named(rdev::Key::Space) => vec![VK_SPACE],
            ShortcutKey::Named(rdev::Key::Return) => vec![VK_RETURN],
            ShortcutKey::Named(rdev::Key::Tab) => vec![VK_TAB],
            ShortcutKey::Named(rdev::Key::Escape) => vec![VK_ESCAPE],
            ShortcutKey::Named(rdev::Key::Backspace) => vec![VK_BACK],
            ShortcutKey::Named(rdev::Key::Delete) => vec![VK_DELETE],
            ShortcutKey::Named(rdev::Key::UpArrow) => vec![VK_UP],
            ShortcutKey::Named(rdev::Key::DownArrow) => vec![VK_DOWN],
            ShortcutKey::Named(rdev::Key::LeftArrow) => vec![VK_LEFT],
            ShortcutKey::Named(rdev::Key::RightArrow) => vec![VK_RIGHT],
            ShortcutKey::Named(rdev::Key::KeyA) => vec![VK_A],
            ShortcutKey::Named(rdev::Key::KeyB) => vec![VK_B],
            ShortcutKey::Named(rdev::Key::KeyC) => vec![VK_C],
            ShortcutKey::Named(rdev::Key::KeyD) => vec![VK_D],
            ShortcutKey::Named(rdev::Key::KeyE) => vec![VK_E],
            ShortcutKey::Named(rdev::Key::KeyF) => vec![VK_F],
            ShortcutKey::Named(rdev::Key::KeyG) => vec![VK_G],
            ShortcutKey::Named(rdev::Key::KeyH) => vec![VK_H],
            ShortcutKey::Named(rdev::Key::KeyI) => vec![VK_I],
            ShortcutKey::Named(rdev::Key::KeyJ) => vec![VK_J],
            ShortcutKey::Named(rdev::Key::KeyK) => vec![VK_K],
            ShortcutKey::Named(rdev::Key::KeyL) => vec![VK_L],
            ShortcutKey::Named(rdev::Key::KeyM) => vec![VK_M],
            ShortcutKey::Named(rdev::Key::KeyN) => vec![VK_N],
            ShortcutKey::Named(rdev::Key::KeyO) => vec![VK_O],
            ShortcutKey::Named(rdev::Key::KeyP) => vec![VK_P],
            ShortcutKey::Named(rdev::Key::KeyQ) => vec![VK_Q],
            ShortcutKey::Named(rdev::Key::KeyR) => vec![VK_R],
            ShortcutKey::Named(rdev::Key::KeyS) => vec![VK_S],
            ShortcutKey::Named(rdev::Key::KeyT) => vec![VK_T],
            ShortcutKey::Named(rdev::Key::KeyU) => vec![VK_U],
            ShortcutKey::Named(rdev::Key::KeyV) => vec![VK_V],
            ShortcutKey::Named(rdev::Key::KeyW) => vec![VK_W],
            ShortcutKey::Named(rdev::Key::KeyX) => vec![VK_X],
            ShortcutKey::Named(rdev::Key::KeyY) => vec![VK_Y],
            ShortcutKey::Named(rdev::Key::KeyZ) => vec![VK_Z],
            ShortcutKey::Named(_) => Vec::new(),
        })
        .collect::<Vec<_>>();

    let started_at = std::time::Instant::now();
    while started_at.elapsed() < timeout {
        let still_pressed = virtual_keys
            .iter()
            .any(|vk| unsafe { GetAsyncKeyState(*vk as i32) } < 0);
        if !still_pressed {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(5));
    }

    Ok(())
}

impl KeyboardShortcutListener {
    pub fn register(shortcut: &str) -> Result<Self> {
        let required_keys = parse_shortcut(shortcut)?;
        let trigger_rx = spawn_listener(required_keys);
        Ok(Self { trigger_rx })
    }

    pub fn recv(&self) -> Result<()> {
        self.trigger_rx
            .recv()
            .context("keyboard listener disconnected")
    }

    pub fn recv_timeout(&self, duration: Duration) -> Result<Option<()>> {
        match self.trigger_rx.recv_timeout(duration) {
            Ok(()) => Ok(Some(())),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                anyhow::bail!("keyboard listener disconnected")
            }
        }
    }
}

fn spawn_listener(required_keys: Vec<ShortcutKey>) -> Receiver<()> {
    let (tx, rx) = mpsc::channel();
    let state = Arc::new(Mutex::new(ListenerState {
        pressed: HashSet::new(),
        triggered: false,
    }));
    let listener_state = Arc::clone(&state);

    thread::spawn(move || {
        let callback = move |event: Event| {
            let mut state = match listener_state.lock() {
                Ok(value) => value,
                Err(_) => return,
            };

            match event.event_type {
                EventType::KeyPress(key) => {
                    state.pressed.insert(key);
                    if is_shortcut_active(&required_keys, &state.pressed) && !state.triggered {
                        state.triggered = true;
                        let _ = tx.send(());
                    }
                }
                EventType::KeyRelease(key) => {
                    state.pressed.remove(&key);
                    if !is_shortcut_active(&required_keys, &state.pressed) {
                        state.triggered = false;
                    }
                }
                _ => {}
            }
        };

        let _ = rdev::listen(callback);
    });

    rx
}

fn parse_shortcut(shortcut: &str) -> Result<Vec<ShortcutKey>> {
    let required = shortcut
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(parse_shortcut_part)
        .collect::<Result<Vec<_>>>()?;

    if required.is_empty() {
        bail!("shortcut must contain at least one key");
    }

    Ok(required)
}

fn parse_shortcut_part(part: &str) -> Result<ShortcutKey> {
    let normalized = part.to_ascii_lowercase();
    let key = match normalized.as_str() {
        "ctrl" | "control" => ShortcutKey::Control,
        "shift" => ShortcutKey::Shift,
        "alt" => ShortcutKey::Alt,
        "win" | "meta" | "super" | "cmd" | "command" => ShortcutKey::Meta,
        "space" => ShortcutKey::Named(rdev::Key::Space),
        "enter" | "return" => ShortcutKey::Named(rdev::Key::Return),
        "tab" => ShortcutKey::Named(rdev::Key::Tab),
        "esc" | "escape" => ShortcutKey::Named(rdev::Key::Escape),
        "backspace" => ShortcutKey::Named(rdev::Key::Backspace),
        "delete" => ShortcutKey::Named(rdev::Key::Delete),
        "up" => ShortcutKey::Named(rdev::Key::UpArrow),
        "down" => ShortcutKey::Named(rdev::Key::DownArrow),
        "left" => ShortcutKey::Named(rdev::Key::LeftArrow),
        "right" => ShortcutKey::Named(rdev::Key::RightArrow),
        _ => parse_named_key(&normalized)?,
    };
    Ok(key)
}

fn parse_named_key(part: &str) -> Result<ShortcutKey> {
    let key = match part {
        "a" => rdev::Key::KeyA,
        "b" => rdev::Key::KeyB,
        "c" => rdev::Key::KeyC,
        "d" => rdev::Key::KeyD,
        "e" => rdev::Key::KeyE,
        "f" => rdev::Key::KeyF,
        "g" => rdev::Key::KeyG,
        "h" => rdev::Key::KeyH,
        "i" => rdev::Key::KeyI,
        "j" => rdev::Key::KeyJ,
        "k" => rdev::Key::KeyK,
        "l" => rdev::Key::KeyL,
        "m" => rdev::Key::KeyM,
        "n" => rdev::Key::KeyN,
        "o" => rdev::Key::KeyO,
        "p" => rdev::Key::KeyP,
        "q" => rdev::Key::KeyQ,
        "r" => rdev::Key::KeyR,
        "s" => rdev::Key::KeyS,
        "t" => rdev::Key::KeyT,
        "u" => rdev::Key::KeyU,
        "v" => rdev::Key::KeyV,
        "w" => rdev::Key::KeyW,
        "x" => rdev::Key::KeyX,
        "y" => rdev::Key::KeyY,
        "z" => rdev::Key::KeyZ,
        "0" => rdev::Key::Num0,
        "1" => rdev::Key::Num1,
        "2" => rdev::Key::Num2,
        "3" => rdev::Key::Num3,
        "4" => rdev::Key::Num4,
        "5" => rdev::Key::Num5,
        "6" => rdev::Key::Num6,
        "7" => rdev::Key::Num7,
        "8" => rdev::Key::Num8,
        "9" => rdev::Key::Num9,
        _ => bail!("unsupported shortcut key: {part}"),
    };

    Ok(ShortcutKey::Named(key))
}

fn is_shortcut_active(required: &[ShortcutKey], pressed: &HashSet<rdev::Key>) -> bool {
    required.iter().all(|required_key| {
        pressed
            .iter()
            .copied()
            .any(|pressed_key| shortcut_key_matches(required_key, pressed_key))
    })
}

fn shortcut_key_matches(required: &ShortcutKey, pressed: rdev::Key) -> bool {
    match required {
        ShortcutKey::Control => matches!(pressed, rdev::Key::ControlLeft | rdev::Key::ControlRight),
        ShortcutKey::Shift => matches!(pressed, rdev::Key::ShiftLeft | rdev::Key::ShiftRight),
        ShortcutKey::Alt => matches!(pressed, rdev::Key::Alt | rdev::Key::AltGr),
        ShortcutKey::Meta => matches!(pressed, rdev::Key::MetaLeft | rdev::Key::MetaRight),
        ShortcutKey::Named(expected) => *expected == pressed,
    }
}

fn parse_send_combination(combination: &str) -> Result<Vec<Key>> {
    combination
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            let normalized = part.to_ascii_lowercase();
            let key = match normalized.as_str() {
                "ctrl" | "control" => Key::Control,
                "shift" => Key::Shift,
                "alt" => Key::Alt,
                "meta" | "win" | "super" | "cmd" | "command" => Key::Meta,
                "space" => Key::Space,
                "enter" | "return" => Key::Return,
                "tab" => Key::Tab,
                "v" => Key::Unicode('v'),
                other if other.len() == 1 => {
                    let ch = other.chars().next().expect("single character");
                    Key::Unicode(ch)
                }
                _ => bail!("unsupported send key: {part}"),
            };
            Ok(key)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifier_only_shortcut() {
        let shortcut = parse_shortcut("ctrl+shift").unwrap();
        assert_eq!(shortcut, vec![ShortcutKey::Control, ShortcutKey::Shift]);
    }

    #[test]
    fn parses_shortcut_with_named_key() {
        let shortcut = parse_shortcut("ctrl+shift+space").unwrap();
        assert_eq!(
            shortcut,
            vec![
                ShortcutKey::Control,
                ShortcutKey::Shift,
                ShortcutKey::Named(rdev::Key::Space)
            ]
        );
    }

    #[test]
    fn send_combination_supports_paste_shortcuts() {
        let keys = parse_send_combination("ctrl+shift+v").unwrap();
        assert_eq!(keys, vec![Key::Control, Key::Shift, Key::Unicode('v')]);
    }
}
