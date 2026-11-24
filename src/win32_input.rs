//! Native Win32 Process-Aware Foreground Detection, Own-PID Window Resolver (`EnumWindows`),
//! Low-Level Keyboard Hook (`WH_KEYBOARD_LL`), Instant Hidden-Window Wakeup (`SW_SHOW` + `AttachThreadInput`
//! + `GetDpiForWindow` + `InvalidateRect` + `egui::Context::request_repaint`), Synthetic Keystroke Sender (`SendInput`),
//! and Direct Menu-Bar Clicker (`GetMenu` / `WM_COMMAND`).

use eframe::egui;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveHostApp {
    PremierePro,
    ArzPalette,
    ArzSettingsModal,
    Other,
}

#[derive(Debug, Clone)]
pub enum HotkeyEvent {
    ShowPalette { premiere_hwnd: isize, window_title: String },
    HidePalette,
    DirectShortcut { key_code: u16, ctrl: bool, alt: bool, shift: bool, premiere_hwnd: isize },
    RecordedToggleShortcut { key_code: u16, ctrl: bool, alt: bool, shift: bool },
}

static HOOK_INTERNAL_TX: OnceLock<mpsc::Sender<RawHookSignal>> = OnceLock::new();
static OWN_ARZ_HWND: AtomicIsize = AtomicIsize::new(0);
static LAST_PREMIERE_HWND: AtomicIsize = AtomicIsize::new(0);
static PACKED_TOGGLE_SHORTCUT: AtomicU32 = AtomicU32::new(0x0020 | (1 << 17));
static RECORDING_SHORTCUT: AtomicBool = AtomicBool::new(false);
static PALETTE_VISIBLE: AtomicBool = AtomicBool::new(false);
static ALLOW_ANY_FOREGROUND_FOR_TEST: AtomicBool = AtomicBool::new(false);
static HOOK_CTRL_HELD: AtomicBool = AtomicBool::new(false);
static HOOK_ALT_HELD: AtomicBool = AtomicBool::new(false);
static HOOK_SHIFT_HELD: AtomicBool = AtomicBool::new(false);
static HOOK_PUMP_TID: AtomicU32 = AtomicU32::new(0);
static START_MONO: OnceLock<Instant> = OnceLock::new();
static LAST_TOGGLE_MS: AtomicU64 = AtomicU64::new(0);

fn monotonic_ms() -> u64 {
    START_MONO.get_or_init(Instant::now).elapsed().as_millis() as u64
}

fn try_claim_toggle_debounce() -> bool {
    let now = monotonic_ms().max(300);
    let prev = LAST_TOGGLE_MS.load(Ordering::SeqCst);
    if prev != 0 && now.saturating_sub(prev) < 220 {
        return false;
    }
    LAST_TOGGLE_MS.store(now, Ordering::SeqCst);
    true
}

#[derive(Debug, Clone)]
enum RawHookSignal {
    TogglePressed { fg_hwnd: isize },
    ForceOpen,
    CandidateDirect { vk: u16, ctrl: bool, alt: bool, shift: bool, fg_hwnd: isize },
    Recorded { vk: u16, ctrl: bool, alt: bool, shift: bool },
}

pub fn set_allow_any_foreground_for_test(allow: bool) {
    ALLOW_ANY_FOREGROUND_FOR_TEST.store(allow, Ordering::SeqCst);
}

pub fn set_palette_visible_flag(vis: bool) {
    PALETTE_VISIBLE.store(vis, Ordering::SeqCst);
}

pub fn is_palette_visible_flag() -> bool {
    PALETTE_VISIBLE.load(Ordering::SeqCst)
}

pub fn request_open_palette_external() {
    if let Some(tx) = HOOK_INTERNAL_TX.get() {
        let _ = tx.send(RawHookSignal::ForceOpen);
    }
}

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
    PACKED_TOGGLE_SHORTCUT.store(packed, Ordering::SeqCst);
}

pub fn set_shortcut_recording_mode(recording: bool) {
    RECORDING_SHORTCUT.store(recording, Ordering::SeqCst);
}

pub fn last_premiere_hwnd() -> isize {
    LAST_PREMIERE_HWND.load(Ordering::SeqCst)
}

pub fn hook_pump_thread_id() -> u32 {
    HOOK_PUMP_TID.load(Ordering::SeqCst)
}

#[cfg(windows)]
mod win_impl {
    use super::*;
    use std::thread;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{BOOL, CloseHandle, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::InvalidateRect;
    use windows::Win32::System::Threading::{
        AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
        QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, RegisterHotKey, SendInput, SetFocus, UnregisterHotKey, HOT_KEY_MODIFIERS,
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL,
        MOD_NOREPEAT, MOD_SHIFT, VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU,
        VK_LSHIFT, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_SHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, CallNextHookEx, DispatchMessageW, EnumWindows, GetAncestor,
        GetForegroundWindow, GetMenu, GetMenuItemCount, GetMenuItemID, GetMenuStringW, GetMessageW,
        GetSubMenu, GetSystemMetrics, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
        IsWindow, PostMessageW, PostThreadMessageW, SetForegroundWindow, SetWindowPos,
        SetWindowsHookExW, ShowWindow, TranslateMessage, GA_ROOT, HMENU, HWND_TOPMOST,
        KBDLLHOOKSTRUCT, LLKHF_ALTDOWN, MF_BYPOSITION, MSG, SM_CXSCREEN, SM_CYSCREEN,
        SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WH_KEYBOARD_LL, WM_COMMAND, WM_HOTKEY, WM_KEYDOWN,
        WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_USER,
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

    unsafe extern "system" fn enum_own_window_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let target_pid = lparam.0 as u32;
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == target_pid && window_title_of(hwnd) == "ARZ-Excalibur" {
            OWN_ARZ_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
            return BOOL(0);
        }
        BOOL(1)
    }

    /// Resolves ONLY the `HWND` belonging to our own `ARZ-Excalibur.exe` process (`GetCurrentProcessId()`),
    /// so File Explorer windows or browser tabs named `"ARZ-Excalibur"` can NEVER collide with our HWND!
    pub fn find_own_arz_hwnd() -> Option<HWND> {
        unsafe {
            let cached = OWN_ARZ_HWND.load(Ordering::SeqCst);
            if cached != 0 {
                let h = HWND(cached as *mut core::ffi::c_void);
                if IsWindow(h).as_bool() {
                    return Some(h);
                }
                OWN_ARZ_HWND.store(0, Ordering::SeqCst);
            }
            let my_pid = GetCurrentProcessId();
            let _ = EnumWindows(Some(enum_own_window_proc), LPARAM(my_pid as isize));
            let found = OWN_ARZ_HWND.load(Ordering::SeqCst);
            if found != 0 {
                Some(HWND(found as *mut core::ffi::c_void))
            } else {
                None
            }
        }
    }

    fn process_exe_path_of(hwnd: HWND) -> (u32, String) {
        if hwnd.0.is_null() {
            return (0, String::new());
        }
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return (0, String::new());
            }
            let Ok(hproc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return (pid, String::new());
            };
            let mut buf = [0u16; 1024];
            let mut sz = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                hproc,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut sz,
            );
            let _ = CloseHandle(hproc);
            if ok.is_ok() && sz > 0 {
                (pid, String::from_utf16_lossy(&buf[..sz as usize]))
            } else {
                (pid, String::new())
            }
        }
    }

    /// Classifies any window accurately by BOTH owning process `.exe` path AND window title,
    /// so undocked Premiere 2027 timelines, monitors, bins, and panels ALWAYS match!
    pub fn classify_window(hwnd: HWND) -> (ActiveHostApp, String) {
        if hwnd.0.is_null() {
            if ALLOW_ANY_FOREGROUND_FOR_TEST.load(Ordering::SeqCst) {
                return (ActiveHostApp::PremierePro, "Test Host".into());
            }
            let last_p = LAST_PREMIERE_HWND.load(Ordering::SeqCst);
            if last_p != 0 {
                let p_hwnd = HWND(last_p as *mut core::ffi::c_void);
                if unsafe { IsWindow(p_hwnd).as_bool() && windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(p_hwnd).as_bool() } {
                    return (ActiveHostApp::PremierePro, "Adobe Premiere Pro".into());
                }
            }
            return (ActiveHostApp::Other, String::new());
        }
        let title = window_title_of(hwnd);
        let title_low = title.to_lowercase();

        // Never swallow keys when user is typing a new shortcut inside ARZ-Excalibur Settings modal!
        if title_low.contains("arz-excalibur settings") {
            return (ActiveHostApp::ArzSettingsModal, title);
        }

        let (pid, exe_path) = process_exe_path_of(hwnd);
        let my_pid = unsafe { GetCurrentProcessId() };
        if pid == my_pid {
            return (ActiveHostApp::ArzPalette, title);
        }

        let exe_low = exe_path.to_lowercase();
        let is_premiere_proc = exe_low.contains("adobe premiere")
            || exe_low.ends_with("premiere pro.exe")
            || exe_low.contains("cephtmlengine")
            || title_low.contains("adobe premiere")
            || title_low.starts_with("premiere pro");

        if is_premiere_proc {
            unsafe {
                let root = GetAncestor(hwnd, GA_ROOT);
                let best = if !root.0.is_null() { root } else { hwnd };
                if !exe_low.contains("cephtmlengine") {
                    LAST_PREMIERE_HWND.store(best.0 as isize, Ordering::SeqCst);
                }
            }
            let display_title = if !title.is_empty() {
                title
            } else {
                "Adobe Premiere Pro".to_string()
            };
            return (ActiveHostApp::PremierePro, display_title);
        }

        if ALLOW_ANY_FOREGROUND_FOR_TEST.load(Ordering::SeqCst) {
            LAST_PREMIERE_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
            return (ActiveHostApp::PremierePro, title);
        }

        (ActiveHostApp::Other, title)
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
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let vk = kb.vkCode as u16;
            let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

            if is_down || is_up {
                match VIRTUAL_KEY(vk) {
                    VK_CONTROL | VK_LCONTROL | VK_RCONTROL => {
                        HOOK_CTRL_HELD.store(is_down, Ordering::SeqCst);
                    }
                    VK_MENU | VK_LMENU | VK_RMENU => {
                        HOOK_ALT_HELD.store(is_down, Ordering::SeqCst);
                    }
                    VK_SHIFT | VK_LSHIFT | VK_RSHIFT => {
                        HOOK_SHIFT_HELD.store(is_down, Ordering::SeqCst);
                    }
                    _ => {}
                }
            }

            if is_down {
                let alt_down = HOOK_ALT_HELD.load(Ordering::SeqCst)
                    || (kb.flags.0 & LLKHF_ALTDOWN.0) != 0
                    || (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0
                    || (GetAsyncKeyState(VK_LMENU.0 as i32) as u16 & 0x8000) != 0
                    || (GetAsyncKeyState(VK_RMENU.0 as i32) as u16 & 0x8000) != 0;
                let ctrl_down = HOOK_CTRL_HELD.load(Ordering::SeqCst)
                    || (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0
                    || (GetAsyncKeyState(VK_LCONTROL.0 as i32) as u16 & 0x8000) != 0
                    || (GetAsyncKeyState(VK_RCONTROL.0 as i32) as u16 & 0x8000) != 0;
                let shift_down = HOOK_SHIFT_HELD.load(Ordering::SeqCst)
                    || (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0
                    || (GetAsyncKeyState(VK_LSHIFT.0 as i32) as u16 & 0x8000) != 0
                    || (GetAsyncKeyState(VK_RSHIFT.0 as i32) as u16 & 0x8000) != 0;

                // 1. Live Shortcut Recording inside ARZ-Excalibur Palette
                if RECORDING_SHORTCUT.load(Ordering::SeqCst) {
                    let is_modifier_only = matches!(
                        VIRTUAL_KEY(vk),
                        VK_CONTROL
                            | VK_LCONTROL
                            | VK_RCONTROL
                            | VK_MENU
                            | VK_LMENU
                            | VK_RMENU
                            | VK_SHIFT
                            | VK_LSHIFT
                            | VK_RSHIFT
                    );
                    if vk == VK_ESCAPE.0 {
                        RECORDING_SHORTCUT.store(false, Ordering::SeqCst);
                    } else if !is_modifier_only {
                        RECORDING_SHORTCUT.store(false, Ordering::SeqCst);
                        set_toggle_shortcut_binding(vk, ctrl_down, alt_down, shift_down);
                        if let Some(tx) = HOOK_INTERNAL_TX.get() {
                            let _ = tx.send(RawHookSignal::Recorded {
                                vk,
                                ctrl: ctrl_down,
                                alt: alt_down,
                                shift: shift_down,
                            });
                        }
                        return LRESULT(1);
                    }
                }

                // 2. Dynamic Toggle Shortcut check
                let packed = PACKED_TOGGLE_SHORTCUT.load(Ordering::SeqCst);
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
                    let (host, _) = classify_window(fg);
                    if host == ActiveHostApp::PremierePro || host == ActiveHostApp::ArzPalette {
                        if try_claim_toggle_debounce() {
                            if let Some(tx) = HOOK_INTERNAL_TX.get() {
                                let _ = tx.send(RawHookSignal::TogglePressed {
                                    fg_hwnd: fg.0 as isize,
                                });
                            }
                        }
                        return LRESULT(1);
                    }
                } else if (alt_down || ctrl_down) && vk >= 0x30 {
                    let fg = GetForegroundWindow();
                    let (host, _) = classify_window(fg);
                    if host == ActiveHostApp::PremierePro {
                        if let Some(tx) = HOOK_INTERNAL_TX.get() {
                            let _ = tx.send(RawHookSignal::CandidateDirect {
                                vk,
                                ctrl: ctrl_down,
                                alt: alt_down,
                                shift: shift_down,
                                fg_hwnd: fg.0 as isize,
                            });
                        }
                    }
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    /// Starts BOTH:
    /// 1) The low-level Win32 keyboard hook thread (`GetMessageW` pump), and
    /// 2) A dedicated instant wakeup dispatcher thread that queues `HotkeyEvent::ShowPalette` first,
    ///    then calls `show_and_center_arz_window` + `egui_ctx.request_repaint()`, bypassing the `SW_HIDE` freeze!
    pub fn start_hotkey_listener(
        egui_ctx: egui::Context,
        bar_w: i32,
        bar_h: i32,
    ) -> mpsc::Receiver<HotkeyEvent> {
        let (app_tx, app_rx) = mpsc::channel::<HotkeyEvent>();
        let (raw_tx, raw_rx) = mpsc::channel::<RawHookSignal>();
        let _ = HOOK_INTERNAL_TX.set(raw_tx);

        // Thread 1: Win32 WH_KEYBOARD_LL + Dynamic Foreground RegisterHotKey (`WM_HOTKEY`) message pump
        thread::spawn(|| unsafe {
            let _ = START_MONO.set(Instant::now());
            HOOK_PUMP_TID.store(GetCurrentThreadId(), Ordering::SeqCst);
            let _hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                if msg.message == WM_USER + 1 {
                    let _ = UnregisterHotKey(None, 1);
                    let want_reg = msg.wParam.0 != 0;
                    if want_reg {
                        let packed = msg.lParam.0 as u32;
                        let vk = packed & 0xFFFF;
                        let mut mods = MOD_NOREPEAT;
                        if (packed & (1 << 16)) != 0 {
                            mods |= MOD_CONTROL;
                        }
                        if (packed & (1 << 17)) != 0 {
                            mods |= MOD_ALT;
                        }
                        if (packed & (1 << 18)) != 0 {
                            mods |= MOD_SHIFT;
                        }
                        let _ = RegisterHotKey(None, 1, HOT_KEY_MODIFIERS(mods.0), vk);
                    }
                } else if msg.message == WM_HOTKEY && msg.wParam.0 == 1 {
                    let fg = GetForegroundWindow();
                    let (host, _) = classify_window(fg);
                    if (host == ActiveHostApp::PremierePro || host == ActiveHostApp::ArzPalette)
                        && try_claim_toggle_debounce()
                    {
                        if let Some(tx) = HOOK_INTERNAL_TX.get() {
                            let _ = tx.send(RawHookSignal::TogglePressed {
                                fg_hwnd: fg.0 as isize,
                            });
                        }
                    }
                } else {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });

        // Thread 3: Dynamic Foreground Hotkey Scope Sync + 40Hz GetAsyncKeyState Hardware Edge Fallback
        thread::spawn(|| unsafe {
            let mut prev_reg_state: (bool, u32) = (false, 0);
            let mut prev_chord_down = false;
            loop {
                thread::sleep(Duration::from_millis(25));
                let fg = GetForegroundWindow();
                let (host, _) = classify_window(fg);
                let rec = RECORDING_SHORTCUT.load(Ordering::SeqCst);
                let packed = PACKED_TOGGLE_SHORTCUT.load(Ordering::SeqCst);
                let in_scope = !rec
                    && (host == ActiveHostApp::PremierePro || host == ActiveHostApp::ArzPalette);

                let desired = (in_scope, packed);
                if desired != prev_reg_state {
                    let tid = HOOK_PUMP_TID.load(Ordering::SeqCst);
                    if tid != 0
                        && PostThreadMessageW(
                            tid,
                            WM_USER + 1,
                            WPARAM(usize::from(in_scope)),
                            LPARAM(packed as isize),
                        )
                        .is_ok()
                    {
                        prev_reg_state = desired;
                    }
                }

                if in_scope {
                    let target_vk = (packed & 0xFFFF) as i32;
                    let target_ctrl = (packed & (1 << 16)) != 0;
                    let target_alt = (packed & (1 << 17)) != 0;
                    let target_shift = (packed & (1 << 18)) != 0;

                    let key_down = (GetAsyncKeyState(target_vk) as u16 & 0x8000) != 0;
                    let ctrl_down = (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0
                        || (GetAsyncKeyState(VK_LCONTROL.0 as i32) as u16 & 0x8000) != 0
                        || (GetAsyncKeyState(VK_RCONTROL.0 as i32) as u16 & 0x8000) != 0;
                    let alt_down = (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0
                        || (GetAsyncKeyState(VK_LMENU.0 as i32) as u16 & 0x8000) != 0
                        || (GetAsyncKeyState(VK_RMENU.0 as i32) as u16 & 0x8000) != 0;
                    let shift_down = (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0
                        || (GetAsyncKeyState(VK_LSHIFT.0 as i32) as u16 & 0x8000) != 0
                        || (GetAsyncKeyState(VK_RSHIFT.0 as i32) as u16 & 0x8000) != 0;

                    let chord_now = key_down
                        && ctrl_down == target_ctrl
                        && alt_down == target_alt
                        && shift_down == target_shift;
                    if chord_now && !prev_chord_down && try_claim_toggle_debounce() {
                        if let Some(tx) = HOOK_INTERNAL_TX.get() {
                            let _ = tx.send(RawHookSignal::TogglePressed {
                                fg_hwnd: fg.0 as isize,
                            });
                        }
                    }
                    prev_chord_down = chord_now;
                } else {
                    prev_chord_down = false;
                }
            }
        });

        // Thread 2: Instant Window Show/Hide & Repaint Dispatcher (works even while HWND is SW_HIDE!)
        thread::spawn(move || {
            while let Ok(sig) = raw_rx.recv() {
                match sig {
                    RawHookSignal::TogglePressed { fg_hwnd } => {
                        let currently_vis = PALETTE_VISIBLE.load(Ordering::SeqCst);
                        let (fg_host, title) =
                            classify_window(HWND(fg_hwnd as *mut core::ffi::c_void));
                        // Open & focus if currently hidden OR if Premiere had focus while palette was unfocused
                        if !currently_vis || fg_host != ActiveHostApp::ArzPalette {
                            PALETTE_VISIBLE.store(true, Ordering::SeqCst);
                            let p_hwnd = LAST_PREMIERE_HWND.load(Ordering::SeqCst);
                            // Queue ShowPalette BEFORE Win32 SW_SHOW so WM_PAINT never races ahead of hotkey_rx!
                            let _ = app_tx.send(HotkeyEvent::ShowPalette {
                                premiere_hwnd: p_hwnd,
                                window_title: title,
                            });
                            show_and_center_arz_window(bar_w, bar_h);
                            egui_ctx.request_repaint();
                        } else {
                            PALETTE_VISIBLE.store(false, Ordering::SeqCst);
                            let _ = app_tx.send(HotkeyEvent::HidePalette);
                            hide_arz_window();
                            let _ = focus_premiere_window();
                            egui_ctx.request_repaint();
                        }
                    }
                    RawHookSignal::ForceOpen => {
                        PALETTE_VISIBLE.store(true, Ordering::SeqCst);
                        let p_hwnd = LAST_PREMIERE_HWND.load(Ordering::SeqCst);
                        let _ = app_tx.send(HotkeyEvent::ShowPalette {
                            premiere_hwnd: p_hwnd,
                            window_title: "Adobe Premiere Pro".into(),
                        });
                        show_and_center_arz_window(bar_w, bar_h);
                        egui_ctx.request_repaint();
                    }
                    RawHookSignal::CandidateDirect { vk, ctrl, alt, shift, fg_hwnd } => {
                        let _ = app_tx.send(HotkeyEvent::DirectShortcut {
                            key_code: vk,
                            ctrl,
                            alt,
                            shift,
                            premiere_hwnd: fg_hwnd,
                        });
                        egui_ctx.request_repaint();
                    }
                    RawHookSignal::Recorded { vk, ctrl, alt, shift } => {
                        let _ = app_tx.send(HotkeyEvent::RecordedToggleShortcut {
                            key_code: vk,
                            ctrl,
                            alt,
                            shift,
                        });
                        egui_ctx.request_repaint();
                    }
                }
            }
        });

        app_rx
    }

    pub fn hide_arz_window() {
        PALETTE_VISIBLE.store(false, Ordering::SeqCst);
        if let Some(hwnd) = find_own_arz_hwnd() {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
    }

    pub fn show_and_center_arz_window(logical_w: i32, logical_h: i32) {
        PALETTE_VISIBLE.store(true, Ordering::SeqCst);
        let Some(hwnd) = find_own_arz_hwnd() else {
            return;
        };
        unsafe {
            let dpi = GetDpiForWindow(hwnd);
            let scale_dpi = if dpi > 0 { dpi as i32 } else { 96 };
            let width = (logical_w * scale_dpi) / 96;
            let height = (logical_h * scale_dpi) / 96;

            let mut target_x = (GetSystemMetrics(SM_CXSCREEN) - width) / 2;
            let mut target_y = (GetSystemMetrics(SM_CYSCREEN) - height) / 3;

            let p_raw = LAST_PREMIERE_HWND.load(Ordering::SeqCst);
            if p_raw != 0 {
                let p_hwnd = HWND(p_raw as *mut core::ffi::c_void);
                let mut r = RECT::default();
                if GetWindowRect(p_hwnd, &mut r).is_ok() && (r.right - r.left) > 200 {
                    target_x = r.left + ((r.right - r.left) - width) / 2;
                    target_y = r.top + ((r.bottom - r.top) - height) / 3;
                }
            }

            let fg = GetForegroundWindow();
            let fg_tid = GetWindowThreadProcessId(fg, None);
            let cur_tid = GetCurrentThreadId();
            let dst_tid = GetWindowThreadProcessId(hwnd, None);

            let mut attached_fg = false;
            let mut attached_dst = false;
            if fg_tid != 0 && fg_tid != cur_tid {
                attached_fg = AttachThreadInput(fg_tid, cur_tid, true).as_bool();
            }
            if dst_tid != 0 && dst_tid != cur_tid && dst_tid != fg_tid {
                attached_dst = AttachThreadInput(cur_tid, dst_tid, true).as_bool();
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
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(hwnd);
            let _ = InvalidateRect(hwnd, None, true);

            if attached_dst {
                let _ = AttachThreadInput(cur_tid, dst_tid, false);
            }
            if attached_fg {
                let _ = AttachThreadInput(fg_tid, cur_tid, false);
            }
        }
    }

    pub fn focus_premiere_window() -> Option<HWND> {
        let raw = LAST_PREMIERE_HWND.load(Ordering::SeqCst);
        if raw == 0 {
            return None;
        }
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        unsafe {
            if !IsWindow(hwnd).as_bool() {
                return None;
            }
            let fg = GetForegroundWindow();
            let fg_tid = GetWindowThreadProcessId(fg, None);
            let cur_tid = GetCurrentThreadId();
            let dst_tid = GetWindowThreadProcessId(hwnd, None);
            let mut attached_fg = false;
            let mut attached_dst = false;
            if fg_tid != 0 && fg_tid != cur_tid {
                attached_fg = AttachThreadInput(fg_tid, cur_tid, true).as_bool();
            }
            if dst_tid != 0 && dst_tid != cur_tid && dst_tid != fg_tid {
                attached_dst = AttachThreadInput(cur_tid, dst_tid, true).as_bool();
            }
            let _ = SetForegroundWindow(hwnd);
            if attached_dst {
                let _ = AttachThreadInput(cur_tid, dst_tid, false);
            }
            if attached_fg {
                let _ = AttachThreadInput(fg_tid, cur_tid, false);
            }
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

    pub fn trigger_premiere_menu(path: &[String]) -> Result<(), String> {
        let hwnd = focus_premiere_window().ok_or_else(|| "Premiere Pro window not active".to_string())?;
        unsafe {
            let root_menu = GetMenu(hwnd);
            if root_menu.0.is_null() {
                return Err("Premiere Pro menu bar unavailable".to_string());
            }
            let cmd_id = find_menu_item_recursive(root_menu, path)
                .ok_or_else(|| format!("Menu item not found: {}", path.join(" > ")))?;
            let ok: Result<(), _> =
                PostMessageW(hwnd, WM_COMMAND, WPARAM(cmd_id as usize), LPARAM(0));
            ok.map_err(|e| format!("PostMessageW failed: {e}"))
        }
    }
}

#[cfg(windows)]
pub use win_impl::{
    detect_foreground_host, focus_premiere_window, hide_arz_window, send_key_chord,
    show_and_center_arz_window, start_hotkey_listener, trigger_premiere_menu,
};
