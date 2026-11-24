//! Persistent user configuration stored in `%APPDATA%\ARZ-Excalibur\settings.json`
//! so plugin updates never wipe custom shortcuts, hidden commands, or layout preferences.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Human-readable shortcut, e.g. `"Alt+Space"`, `"Ctrl+Space"`, `"Alt+Q"`, `"F1"`
    pub toggle_shortcut: String,
    pub vk: u16,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// `true` = 3-column Square Cards with 60-FPS Live Preview; `false` = Classic Compact List
    #[serde(default = "default_true")]
    pub grid_mode: bool,
    /// Command IDs hidden by the user in Premiere Settings
    #[serde(default)]
    pub hidden_ids: Vec<String>,
    /// Direct per-command keyboard shortcuts (`cmd_id` -> `"Alt+1"`, `"Ctrl+Shift+S"`, etc.)
    #[serde(default)]
    pub command_shortcuts: HashMap<String, String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            toggle_shortcut: "Alt+Space".into(),
            vk: 0x20,
            ctrl: false,
            alt: true,
            shift: false,
            grid_mode: true,
            hidden_ids: Vec::new(),
            command_shortcuts: HashMap::new(),
        }
    }
}

pub fn user_config_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let dir = PathBuf::from(appdata).join("ARZ-Excalibur");
        let _ = fs::create_dir_all(&dir);
        return dir;
    }
    PathBuf::from(".")
}

pub fn settings_file_path() -> PathBuf {
    user_config_dir().join("settings.json")
}

pub fn macros_file_path() -> PathBuf {
    user_config_dir().join("macros.json")
}

pub fn load_settings() -> AppSettings {
    let path = settings_file_path();
    let raw = fs::read_to_string(&path)
        .or_else(|_| fs::read_to_string("settings.json"))
        .unwrap_or_default();
    if raw.is_empty() {
        let def = AppSettings::default();
        let _ = save_settings(&def);
        return def;
    }
    if let Ok(mut parsed) = serde_json::from_str::<AppSettings>(&raw) {
        if let Some((vk, ctrl, alt, shift)) = parse_shortcut_str(&parsed.toggle_shortcut) {
            parsed.vk = vk;
            parsed.ctrl = ctrl;
            parsed.alt = alt;
            parsed.shift = shift;
        }
        parsed
    } else {
        AppSettings::default()
    }
}

pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    let _ = fs::write(settings_file_path(), &json);
    let _ = fs::write("settings.json", &json);
    Ok(())
}

pub fn vk_to_name(vk: u16) -> String {
    match vk {
        0x20 => "Space".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0xC0 => "`".into(),
        0xBD => "-".into(),
        0xBB => "=".into(),
        0xDB => "[".into(),
        0xDD => "]".into(),
        0xBA => ";".into(),
        0xDE => "'".into(),
        0xBC => ",".into(),
        0xBE => ".".into(),
        0xBF => "/".into(),
        0x70..=0x7B => format!("F{}", vk - 0x70 + 1),
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk as u32).unwrap_or('?').to_string(),
        _ => format!("VK_0x{vk:02X}"),
    }
}

pub fn format_shortcut(vk: u16, ctrl: bool, alt: bool, shift: bool) -> String {
    let mut parts = Vec::new();
    if ctrl {
        parts.push("Ctrl".to_string());
    }
    if alt {
        parts.push("Alt".to_string());
    }
    if shift {
        parts.push("Shift".to_string());
    }
    parts.push(vk_to_name(vk));
    parts.join("+")
}

pub fn parse_shortcut_str(s: &str) -> Option<(u16, bool, bool, bool)> {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut key_vk: Option<u16> = None;

    for tok in s.split('+').map(|t| t.trim()) {
        if tok.is_empty() {
            continue;
        }
        let low = tok.to_lowercase();
        match low.as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "opt" | "option" => alt = true,
            "shift" => shift = true,
            "space" => key_vk = Some(0x20),
            "tab" => key_vk = Some(0x09),
            "`" | "backtick" | "tilde" => key_vk = Some(0xC0),
            "-" | "minus" => key_vk = Some(0xBD),
            "=" | "plus" => key_vk = Some(0xBB),
            "[" => key_vk = Some(0xDB),
            "]" => key_vk = Some(0xDD),
            ";" => key_vk = Some(0xBA),
            "'" => key_vk = Some(0xDE),
            "," => key_vk = Some(0xBC),
            "." => key_vk = Some(0xBE),
            "/" | "slash" => key_vk = Some(0xBF),
            _ if low.starts_with('f')
                && low[1..]
                    .parse::<u16>()
                    .ok()
                    .filter(|&n| (1..=12).contains(&n))
                    .is_some() =>
            {
                let n = low[1..].parse::<u16>().unwrap();
                key_vk = Some(0x70 + n - 1);
            }
            _ if tok.len() == 1 => {
                let ch = tok.chars().next().unwrap().to_ascii_uppercase();
                if ch.is_ascii_alphanumeric() {
                    key_vk = Some(ch as u16);
                }
            }
            _ => {}
        }
    }
    key_vk.map(|vk| (vk, ctrl, alt, shift))
}
