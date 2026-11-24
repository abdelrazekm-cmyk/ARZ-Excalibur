//! User Macro (Multi-step Command) schema, `macros.json` loader, & sequential background runner.

use crate::bridge::{PremiereCommand, SharedBridgeState};
use crate::win32_input::{send_key_chord, trigger_premiere_menu};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "step_type", rename_all = "snake_case")]
pub enum MacroStep {
    /// Enqueue a Premiere Pro ExtendScript bridge command
    Premiere(PremiereCommand),
    /// Synthesize a keyboard shortcut directly into Premiere Pro (`SendInput`)
    KeyShortcut {
        key_code: u16,
        ctrl: bool,
        alt: bool,
        shift: bool,
    },
    /// Click a top-level Premiere Pro menu path via Win32 `WM_COMMAND`
    MenuClick {
        menu_path: Vec<String>,
    },
    /// Explicit pause in milliseconds between steps
    WaitMs {
        ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMacro {
    pub id: String,
    pub name: String,
    pub shortcut_vk: Option<u16>,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub steps: Vec<MacroStep>,
}

pub fn load_user_macros(path: &Path) -> Vec<UserMacro> {
    let raw = fs::read_to_string(crate::settings::macros_file_path())
        .or_else(|_| fs::read_to_string(path))
        .unwrap_or_default();
    if raw.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str::<Vec<UserMacro>>(&raw).unwrap_or_default()
}

pub fn execute_user_macro_async(user_macro: UserMacro, bridge: SharedBridgeState) {
    thread::spawn(move || {
        for step in user_macro.steps {
            match step {
                MacroStep::Premiere(cmd) => {
                    if let Ok(mut guard) = bridge.lock() {
                        guard.pending_commands.push_back(cmd);
                    }
                }
                MacroStep::KeyShortcut { key_code, ctrl, alt, shift } => {
                    send_key_chord(key_code, ctrl, alt, shift);
                }
                MacroStep::MenuClick { menu_path } => {
                    let _ = trigger_premiere_menu(&menu_path);
                }
                MacroStep::WaitMs { ms } => {
                    thread::sleep(Duration::from_millis(ms));
                }
            }
        }
    });
}
