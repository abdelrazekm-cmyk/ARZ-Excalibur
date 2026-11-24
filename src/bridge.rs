//! Loopback-only (`127.0.0.1:49872`) JSON Bridge between `ARZ-Excalibur.exe` and
//! Premiere Pro's invisible `bridge.html` + full 2-Tab `settings.html` UI.

use crate::macro_engine::UserMacro;
use crate::premiere_presets::CompiledPremierePreset;
use crate::search::CommandEntry;
use crate::settings::{
    load_settings, macros_file_path, parse_shortcut_str, save_settings, AppSettings,
};
use crate::win32_input::{
    hook_pump_thread_id, request_open_palette_external, set_toggle_shortcut_binding,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const BRIDGE_ADDR: &str = "127.0.0.1:49872";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PremiereCommand {
    CapturePreviewFrame {
        output_path: String,
    },
    SetClipProperty {
        component: String,
        param: String,
        op: String,
        value: f64,
        axis: Option<usize>,
    },
    ApplyEffect {
        name: String,
        is_video: bool,
    },
    ApplyTransition {
        name: String,
        is_video: bool,
    },
    ApplyCompiledPreset {
        preset: CompiledPremierePreset,
    },
    NestClips {
        sequence_name: String,
        individual: bool,
    },
    TrimToPlayhead {
        trim_in: bool,
    },
    MoveClipToPlayhead {
        align_start: bool,
    },
    AddMarker {
        name: String,
        color_index: u8,
        on_clip: bool,
    },
    MovePlayhead {
        delta_frames: i64,
    },
    SelectClipsRelativeToPlayhead {
        after: bool,
    },
    SetTracksState {
        track_kind: String,
        field: String,
        toggle: bool,
    },
    RunJsxFile {
        path: String,
    },
}

#[derive(Default)]
pub struct BridgeState {
    pub pending_commands: VecDeque<PremiereCommand>,
    pub last_premiere_reply: Option<String>,
    pub connected_ppro_version: Option<String>,
    pub catalog_snapshot: Vec<CommandEntry>,
    pub macros_snapshot: Vec<UserMacro>,
    pub reload_settings_flag: bool,
    pub reload_macros_flag: bool,
    pub last_poll_instant: Option<Instant>,
    pub shutdown_requested: bool,
}

pub type SharedBridgeState = Arc<Mutex<BridgeState>>;

pub fn notify_running_instance_to_open() {
    if let Ok(mut stream) = TcpStream::connect_timeout(
        &"127.0.0.1:49872".parse().unwrap(),
        Duration::from_millis(400),
    ) {
        let _ = stream.write_all(b"POST /open HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let _ = stream.flush();
    }
}

pub fn try_start_loopback_bridge_server(managed_by_premiere: bool) -> Option<SharedBridgeState> {
    let listener = TcpListener::bind(BRIDGE_ADDR).ok()?;
    let state: SharedBridgeState = Arc::new(Mutex::new(BridgeState::default()));
    let state_clone = Arc::clone(&state);

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state_req = Arc::clone(&state_clone);
            let _ = handle_http_conn(stream, state_req);
        }
    });

    // Independent background watchdog so shutdown works even while the UI window is SW_HIDE!
    let state_wd = Arc::clone(&state);
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(500));
        if let Ok(guard) = state_wd.lock() {
            if guard.shutdown_requested {
                std::process::exit(0);
            }
            if managed_by_premiere {
                if let Some(last_ping) = guard.last_poll_instant {
                    if last_ping.elapsed() > Duration::from_secs(5) {
                        std::process::exit(0);
                    }
                }
            }
        }
    });

    Some(state)
}

fn save_macros_file(macros: &[UserMacro]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(macros).map_err(|e| e.to_string())?;
    let _ = fs::write(macros_file_path(), &json);
    let _ = fs::write("macros.json", &json);
    Ok(())
}

/// Reads complete HTTP/1.1 headers + full `Content-Length` body so split TCP packets from CEF `fetch()` never truncate!
fn read_full_http_request(stream: &mut TcpStream) -> std::io::Result<(String, String)> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut data: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk = [0u8; 8192];
    let mut header_end: Option<usize> = None;
    let mut content_len: usize = 0;

    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => break,
            Err(e) => return Err(e),
        };
        data.extend_from_slice(&chunk[..n]);

        if header_end.is_none() {
            if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                let end = pos + 4;
                header_end = Some(end);
                let hdr_str = String::from_utf8_lossy(&data[..pos]);
                for line in hdr_str.lines() {
                    let low = line.to_ascii_lowercase();
                    if let Some(rest) = low.strip_prefix("content-length:") {
                        content_len = rest.trim().parse::<usize>().unwrap_or(0);
                    }
                }
            }
        }

        if let Some(end) = header_end {
            if data.len() >= end + content_len {
                break;
            }
        }
        if data.len() > 2 * 1024 * 1024 {
            break;
        }
    }

    if data.is_empty() {
        return Ok((String::new(), String::new()));
    }
    if let Some(end) = header_end {
        let headers = String::from_utf8_lossy(&data[..end - 4]);
        let first_line = headers.lines().next().unwrap_or("").to_string();
        let body_end = (end + content_len).min(data.len());
        let body = String::from_utf8_lossy(&data[end..body_end]).to_string();
        Ok((first_line, body))
    } else {
        let raw = String::from_utf8_lossy(&data);
        Ok((raw.lines().next().unwrap_or("").to_string(), String::new()))
    }
}

fn handle_http_conn(mut stream: TcpStream, state: SharedBridgeState) -> std::io::Result<()> {
    let (first_line, body) = read_full_http_request(&mut stream)?;
    if first_line.is_empty() {
        return Ok(());
    }

    let json_resp = if first_line.starts_with("GET /poll") {
        let mut guard = state.lock().unwrap();
        guard.last_poll_instant = Some(Instant::now());
        if let Some(ver_idx) = first_line.find("ver=") {
            let ver = first_line[ver_idx + 4..]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            if !ver.is_empty() {
                guard.connected_ppro_version = Some(ver);
            }
        }
        let cmds: Vec<PremiereCommand> = guard.pending_commands.drain(..).collect();
        serde_json::to_string(&cmds).unwrap_or_else(|_| "[]".into())
    } else if first_line.starts_with("GET /state") {
        let settings = load_settings();
        let guard = state.lock().unwrap();
        #[derive(Serialize)]
        struct FullState<'a> {
            settings: AppSettings,
            catalog: &'a [CommandEntry],
            macros: &'a [UserMacro],
            connected_ppro_version: &'a Option<String>,
            hook_tid: u32,
        }
        serde_json::to_string(&FullState {
            settings,
            catalog: &guard.catalog_snapshot,
            macros: &guard.macros_snapshot,
            connected_ppro_version: &guard.connected_ppro_version,
            hook_tid: hook_pump_thread_id(),
        })
        .unwrap_or_else(|_| "{}".into())
    } else if first_line.starts_with("POST /open") || first_line.starts_with("GET /open") {
        request_open_palette_external();
        r#"{"ok":true}"#.to_string()
    } else if first_line.starts_with("POST /settings") {
        if let Ok(mut new_cfg) = serde_json::from_str::<AppSettings>(body.trim()) {
            if let Some((vk, ctrl, alt, shift)) = parse_shortcut_str(&new_cfg.toggle_shortcut) {
                new_cfg.vk = vk;
                new_cfg.ctrl = ctrl;
                new_cfg.alt = alt;
                new_cfg.shift = shift;
                set_toggle_shortcut_binding(vk, ctrl, alt, shift);
            }
            let _ = save_settings(&new_cfg);
            if let Ok(mut guard) = state.lock() {
                guard.reload_settings_flag = true;
            }
            r#"{"ok":true}"#.to_string()
        } else {
            r#"{"ok":false,"error":"Invalid settings JSON"}"#.to_string()
        }
    } else if first_line.starts_with("POST /macros") {
        if let Ok(new_macros) = serde_json::from_str::<Vec<UserMacro>>(body.trim()) {
            let _ = save_macros_file(&new_macros);
            if let Ok(mut guard) = state.lock() {
                guard.macros_snapshot = new_macros;
                guard.reload_macros_flag = true;
            }
            r#"{"ok":true}"#.to_string()
        } else {
            r#"{"ok":false,"error":"Invalid macros JSON"}"#.to_string()
        }
    } else if first_line.starts_with("GET /shortcut") {
        let cur = load_settings();
        serde_json::to_string(&cur).unwrap_or_else(|_| r#"{"toggle_shortcut":"Alt+Space"}"#.into())
    } else if first_line.starts_with("POST /shortcut") {
        #[derive(Deserialize)]
        struct Req {
            toggle_shortcut: String,
        }
        if let Ok(parsed_req) = serde_json::from_str::<Req>(body.trim()) {
            if let Some((vk, ctrl, alt, shift)) = parse_shortcut_str(&parsed_req.toggle_shortcut) {
                set_toggle_shortcut_binding(vk, ctrl, alt, shift);
                let mut cur = load_settings();
                cur.toggle_shortcut = parsed_req.toggle_shortcut;
                cur.vk = vk;
                cur.ctrl = ctrl;
                cur.alt = alt;
                cur.shift = shift;
                let _ = save_settings(&cur);
                if let Ok(mut guard) = state.lock() {
                    guard.reload_settings_flag = true;
                }
                r#"{"ok":true}"#.to_string()
            } else {
                r#"{"ok":false,"error":"Unsupported key"}"#.to_string()
            }
        } else {
            r#"{"ok":false,"error":"Invalid JSON"}"#.to_string()
        }
    } else if first_line.starts_with("POST /shutdown") {
        if let Ok(mut guard) = state.lock() {
            guard.shutdown_requested = true;
        }
        r#"{"ok":true}"#.to_string()
    } else if first_line.starts_with("POST /result") {
        let mut guard = state.lock().unwrap();
        guard.last_premiere_reply = Some(body.trim().to_string());
        r#"{"ok":true}"#.to_string()
    } else if first_line.starts_with("OPTIONS ") {
        String::new()
    } else {
        r#"{"service":"arz-excalibur","status":"ready"}"#.to_string()
    };

    let http = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        json_resp.len(),
        json_resp
    );
    stream.write_all(http.as_bytes())?;
    stream.flush()
}
