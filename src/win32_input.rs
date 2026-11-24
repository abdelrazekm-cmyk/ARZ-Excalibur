//! Native Win32 Foreground Detection, Global Low-Level Keyboard Hook (`WH_KEYBOARD_LL`),
//! Synthetic Keystroke Sender (`SendInput`), and Direct Menu-Bar Clicker (`GetMenu` / `WM_COMMAND`).
//! Replaces both `spell_win.exe` and `spellcast_win.exe` in a single safe Rust module.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{mpsc, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveHostApp {
    PremierePro,
    ArzPalette,
    Other,
}

#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    TogglePalette { premiere_hwnd: isize, window_title: String },
    DirectShortcut { key_code: u16, ctrl: bool, alt: bool, shift: bool, premiere_hwnd: isize },
    RecordedToggleShortcut { key_code: u16, ctrl: bool, alt: bool, shift: bool },
}

static HOOK_TX: OnceLock<mpsc::Sender<HotkeyEvent>> = OnceLock::new();
static LAST_PREMIERE_HWND: AtomicIsize = AtomicIsize::new(0);
// Packed shortcut: bits 0..16 = vk, bit 16 = ctrl, bit 17 = alt, bit 18 = shift. Default: Alt+Space (0x20 | (1<<17))
static PACKED_TOGGLE_SHORTCUT: AtomicU32 = AtomicU32::new(0x0020 | (1 << 17));
static RECORDING_SHORTCUT: AtomicBool = AtomicBool::new(false);

pub fn set_toggle_shortcut_binding(vk: u16, ctrl: bool, alt: bool, shift: bool) {
    let mut packed = vk as u32;
    if ctrl {
        packed |= 1 << 16;
    }
    if alt {
        packed |= 1 << 17;
    }
    if shift {
        packed |= 1 << 18;
    }
    PACKED_TOGGLE_SHORTCUT.store(packed, Ordering::Relaxed);
}

pub fn set_shortcut_recording_mode(recording: bool) {
    RECORDING_SHORTCUT.store(recording, Ordering::Relaxed);
}

pub fn last_premiere_hwnd() -> isize {
    LAST_PREMIERE_HWND.load(Ordering::Relaxed)
}

#[cfg(windows)]
mod win_impl {
    use super::*;
    use std::thread;
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, SetFocus, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
        VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_SHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, FindWindowW, GetForegroundWindow, GetMenu,
        GetMenuItemCount, GetMenuItemID, GetMenuStringW, GetMessageW, GetSubMenu, GetSystemMetrics,
        GetWindowRect, GetWindowTextW, PostMessageW, SetForegroundWindow, SetWindowPos,
        SetWindowsHookExW, ShowWindow, TranslateMessage, HMENU, HWND_TOPMOST, KBDLLHOOKSTRUCT,
        MF_BYPOSITION, MSG, SM_CXSCREEN, SM_CYSCREEN, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW,
        WH_KEYBOARD_LL, WM_COMMAND, WM_KEYDOWN, WM_SYSKEYDOWN,
    };

    pub fn window_title_of(hwnd: HWND) -> String {
        if hwnd.0.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 512];
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
        if len <= 0 {
            String::new()
        } else {
            String::from_utf16_lossy(&buf[..len as usize])
        }
    }

    pub fn classify_window(hwnd: HWND) -> (ActiveHostApp, String) {
        let title = window_title_of(hwnd);
        let lower = title.to_lowercase();
        if lower.contains("adobe premiere") || lower.starts_with("premiere pro") {
            LAST_PREMIERE_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
            (ActiveHostApp::PremierePro, title)
        } else if lower.contains("arz-excalibur") {
            (ActiveHostApp::ArzPalette, title)
        } else {
            (ActiveHostApp::Other, title)
        }
    }

    pub fn detect_foreground_host() -> (ActiveHostApp, String) {
        let hwnd = unsafe { GetForegroundWindow() };
        classify_window(hwnd)
    }

    unsafe extern "system" fn keyboard_hook_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code >= 0 {
            let msg = wparam.0 as u32;
            if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                let vk = kb.vkCode as u16;

                let alt_down = (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0
                    || (GetAsyncKeyState(VK_LMENU.0 as i32) as u16 & 0x8000) != 0
                    || (GetAsyncKeyState(VK_RMENU.0 as i32) as u16 & 0x8000) != 0;
                let ctrl_down = (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
                let shift_down = (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0
                    || (GetAsyncKeyState(VK_LSHIFT.0 as i32) as u16 & 0x8000) != 0;

                // Live Shortcut Recording Mode (when user selects "Change Open Shortcut")
                if RECORDING_SHORTCUT.load(Ordering::Relaxed) {
                    let is_modifier_only = matches!(
                        VIRTUAL_KEY(vk),
                        VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_MENU | VK_LMENU | VK_RMENU | VK_SHIFT | VK_LSHIFT | VK_RSHIFT
                    );
                    if vk == VK_ESCAPE.0 {
                        RECORDING_SHORTCUT.store(false, Ordering::Relaxed);
                    } else if !is_modifier_only {
                        RECORDING_SHORTCUT.store(false, Ordering::Relaxed);
                        set_toggle_shortcut_binding(vk, ctrl_down, alt_down, shift_down);
                        if let Some(tx) = HOOK_TX.get() {
                            let _ = tx.send(HotkeyEvent::RecordedToggleShortcut {
                                key_code: vk,
                                ctrl: ctrl_down,
                                alt: alt_down,
                                shift: shift_down,
                            });
                        }
                        return LRESULT(1);
                    }
                }

                // Dynamic Toggle Shortcut loaded from `settings.json` or live recorder
                let packed = PACKED_TOGGLE_SHORTCUT.load(Ordering::Relaxed);
                let target_vk = (packed & 0xFFFF) as u16;
                let target_ctrl = (packed & (1 << 16)) != 0;
                let target_alt = (packed & (1 << 17)) != 0;
                let target_shift = (packed & (1 << 18)) != 0;

                if vk == target_vk
                    && ctrl_down == target_ctrl
                    && alt_down == target_alt
                    && shift_down == target_shift
                {
                    let fg = GetForegroundWindow();
                    let (host, title) = classify_window(fg);
                    if host == ActiveHostApp::PremierePro || host == ActiveHostApp::ArzPalette {
                        let p_hwnd = LAST_PREMIERE_HWND.load(Ordering::Relaxed);
                        if let Some(tx) = HOOK_TX.get() {
                            let _ = tx.send(HotkeyEvent::TogglePalette {
                                premiere_hwnd: p_hwnd,
                                window_title: title,
                            });
                        }
                        return LRESULT(1);
                    }
                } else if (alt_down || ctrl_down) && vk >= 0x30 {
                    let fg = GetForegroundWindow();
                    let (host, _) = classify_window(fg);
                    if host == ActiveHostApp::PremierePro {
                        if let Some(tx) = HOOK_TX.get() {
                            let _ = tx.send(HotkeyEvent::DirectShortcut {
                                key_code: vk,
                                ctrl: ctrl_down,
                                alt: alt_down,
                                shift: shift_down,
                                premiere_hwnd: fg.0 as isize,
                            });
                        }
                    }
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    pub fn start_hotkey_listener() -> mpsc::Receiver<HotkeyEvent> {
        let (tx, rx) = mpsc::channel();
        let _ = HOOK_TX.set(tx);

        thread::spawn(|| unsafe {
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0);
            if hook.is_ok() {
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).into() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });

        rx
    }

    pub fn hide_arz_window() {
        unsafe {
            if let Ok(hwnd) = FindWindowW(None, w!("ARZ-Excalibur")) {
                if !hwnd.0.is_null() {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            }
        }
    }

    pub fn show_and_center_arz_window(width: i32, height: i32) {
        unsafe {
            let Ok(hwnd) = FindWindowW(None, w!("ARZ-Excalibur")) else {
                return;
            };
            if hwnd.0.is_null() {
                return;
            }

            let mut target_x = (GetSystemMetrics(SM_CXSCREEN) - width) / 2;
            let mut target_y = (GetSystemMetrics(SM_CYSCREEN) - height) / 3;

            let p_raw = LAST_PREMIERE_HWND.load(Ordering::Relaxed);
            if p_raw != 0 {
                let p_hwnd = HWND(p_raw as *mut core::ffi::c_void);
                let mut r = RECT::default();
                if GetWindowRect(p_hwnd, &mut r).is_ok() && (r.right - r.left) > 200 {
                    target_x = r.left + ((r.right - r.left) - width) / 2;
                    target_y = r.top + ((r.bottom - r.top) - height) / 3;
                }
            }

            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                target_x,
                target_y,
                width,
                height,
                SWP_SHOWWINDOW,
            );
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(hwnd);
        }
    }

    pub fn focus_premiere_window() -> Option<HWND> {
        let raw = LAST_PREMIERE_HWND.load(Ordering::Relaxed);
        if raw == 0 {
            return None;
        }
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
        Some(hwnd)
    }

    pub fn send_key_chord(vk: u16, ctrl: bool, alt: bool, shift: bool) {
        let _ = focus_premiere_window();
        let mut inputs: Vec<INPUT> = Vec::with_capacity(8);

        let make_key = |key: u16, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(key),
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };

        if ctrl {
            inputs.push(make_key(VK_CONTROL.0, false));
        }
        if alt {
            inputs.push(make_key(VK_MENU.0, false));
        }
        if shift {
            inputs.push(make_key(VK_SHIFT.0, false));
        }
        inputs.push(make_key(vk, false));
        inputs.push(make_key(vk, true));
        if shift {
            inputs.push(make_key(VK_SHIFT.0, true));
        }
        if alt {
            inputs.push(make_key(VK_MENU.0, true));
        }
        if ctrl {
            inputs.push(make_key(VK_CONTROL.0, true));
        }

        unsafe {
            let _ = SendInput(&inputs, core::mem::size_of::<INPUT>() as i32);
        }
    }

    fn normalize_menu_label(s: &str) -> String {
        let head = s.split('\t').next().unwrap_or(s);
        head.replace('&', "").replace("...", "").trim().to_lowercase()
    }

    unsafe fn find_menu_item_recursive(menu: HMENU, path: &[String]) -> Option<u32> {
        if path.is_empty() || menu.0.is_null() {
            return None;
        }
        let target = normalize_menu_label(&path[0]);
        let count = GetMenuItemCount(menu);
        if count <= 0 {
            return None;
        }

        for idx in 0..count {
            let mut buf = [0u16; 256];
            let len = GetMenuStringW(menu, idx as u32, Some(&mut buf), MF_BYPOSITION);
            if len <= 0 {
                continue;
            }
            let text = String::from_utf16_lossy(&buf[..len as usize]);
            if normalize_menu_label(&text) == target {
                if path.len() == 1 {
                    let id = GetMenuItemID(menu, idx);
                    if id != u32::MAX {
                        return Some(id);
                    }
                } else {
                    let sub = GetSubMenu(menu, idx);
                    if !sub.0.is_null() {
                        return find_menu_item_recursive(sub, &path[1..]);
                    }
                }
            }
        }
        None
    }

    /// Walks Premiere Pro's native Win32 menu bar (`HMENU`) and dispatches `WM_COMMAND`
    /// without moving the cursor (replaces `spellcast_win.exe` `menubclk`).
    pub fn trigger_premiere_menu(path: &[String]) -> Result<(), String> {
        let hwnd = focus_premiere_window()
            .ok_or_else(|| "لم يتم العثور على نافذة Adobe Premiere Pro نشطة".to_string())?;
        unsafe {
            let root_menu = GetMenu(hwnd);
            if root_menu.0.is_null() {
                return Err("شريط قوائم Premiere Pro غير متاح لهذه النافذة".to_string());
            }
            let cmd_id = find_menu_item_recursive(root_menu, path)
                .ok_or_else(|| format!("لم يتم العثور على عنصر القائمة: {}", path.join(" > ")))?;
            let ok: Result<(), _> = PostMessageW(hwnd, WM_COMMAND, WPARAM(cmd_id as usize), LPARAM(0));
            ok.map_err(|e| format!("فشل إرسال أمر القائمة WM_COMMAND: {e}"))
        }
    }
}

#[cfg(windows)]
pub use win_impl::{
    detect_foreground_host, focus_premiere_window, hide_arz_window, send_key_chord,
    show_and_center_arz_window, start_hotkey_listener, trigger_premiere_menu,
};

#[cfg(not(windows))]
pub fn detect_foreground_host() -> (ActiveHostApp, String) {
    (ActiveHostApp::Other, String::new())
}
