#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! ARZ-Excalibur — Native Rust Spotlight Command Bar, Visual 3-Column Effect Grid & Live 60-FPS Preview.
//! Styled in ARZ Studio's official Noir & Brass (#110E0A, #DB9B3E, #F4EEE6) brand identity.

mod bridge;
mod catalog;
mod macro_engine;
mod math_expr;
mod premiere_presets;
mod preview_renderer;
mod search;
mod settings;
mod win32_input;

use bridge::{try_start_loopback_bridge_server, PremiereCommand, SharedBridgeState};
use catalog::{build_premiere_catalog, ActionItem, CommandPayload};
use eframe::egui::{self, Color32, Rounding, Stroke, TextureHandle, Vec2};
use macro_engine::{execute_user_macro_async, load_user_macros, UserMacro};
use math_expr::ValueExpr;
use notify::{RecursiveMode, Watcher};
use premiere_presets::{discover_latest_prfpset_file, parse_prfpset_file};
use preview_renderer::{draw_effect_preview, PreviewFrameBank};
use settings::{format_shortcut, load_settings, parse_shortcut_str, save_settings, AppSettings};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    mpsc::{self, Receiver},
    Arc,
};
use std::time::Duration;
use win32_input::{
    detect_foreground_host, focus_premiere_window, hide_arz_window, last_premiere_hwnd,
    set_shortcut_recording_mode, set_toggle_shortcut_binding, show_and_center_arz_window,
    start_hotkey_listener, trigger_premiere_menu, HotkeyEvent,
};

const GRID_COLS: usize = 3;
const BAR_W: f32 = 500.0;
const BAR_EMPTY_H: f32 = 84.0;

struct ArzExcaliburApp {
    query: String,
    input_primary: String,
    input_secondary: String,
    selected_idx: usize,
    submenu_mode_idx: usize,
    active_submenu: Option<ActionItem>,
    actions: Vec<ActionItem>,
    user_macros: Vec<UserMacro>,
    app_settings: AppSettings,
    bridge: SharedBridgeState,
    hotkey_rx: Receiver<HotkeyEvent>,
    preset_reload_rx: Option<Receiver<()>>,
    _preset_watcher: Option<notify::RecommendedWatcher>,
    watermark_tex: Option<TextureHandle>,
    preview_bank: PreviewFrameBank,
    preview_frame_path: PathBuf,
    custom_previews: HashMap<String, TextureHandle>,
    managed_by_premiere: bool,
    visible: bool,
    status_line: String,
}

fn load_embedded_watermark(ctx: &egui::Context) -> Option<TextureHandle> {
    if let Some(disk_tex) = try_load_png_texture(ctx, "arz_watermark", Path::new("assets/watermark.png")) {
        return Some(disk_tex);
    }
    let bytes = include_bytes!("../assets/watermark.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color_img = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("arz_watermark_embedded", color_img, egui::TextureOptions::LINEAR))
}

fn try_load_png_texture(ctx: &egui::Context, name: &str, path: &Path) -> Option<TextureHandle> {
    let img = image::open(path).ok()?.into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color_img = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture(name.to_string(), color_img, egui::TextureOptions::LINEAR))
}

impl ArzExcaliburApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        bridge: SharedBridgeState,
        start_hidden: bool,
    ) -> Self {
        // Load crisp native Windows Segoe UI font so typography renders razor-sharp
        if let Ok(font_bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "segoe_ui".into(),
                egui::FontData::from_owned(font_bytes).into(),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "segoe_ui".into());
            cc.egui_ctx.set_fonts(fonts);
        }

        // ARZ Studio Official Brand Palette — Noir & Brass (#110E0A, #DB9B3E, #F4EEE6)
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(17, 14, 10); // #110E0A Noir Espresso
        visuals.window_fill = Color32::from_rgb(17, 14, 10);
        visuals.extreme_bg_color = Color32::from_rgb(17, 14, 10);
        visuals.override_text_color = Some(Color32::from_rgb(244, 238, 230)); // #F4EEE6 Warm Ivory
        visuals.selection.bg_fill = Color32::from_rgb(219, 155, 62); // #DB9B3E Warm Brass
        visuals.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(23, 16, 5));
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(82, 60, 29));
        cc.egui_ctx.set_visuals(visuals);

        let app_settings = load_settings();
        set_toggle_shortcut_binding(
            app_settings.vk,
            app_settings.ctrl,
            app_settings.alt,
            app_settings.shift,
        );
        let hotkey_rx = start_hotkey_listener();

        let mut preset_reload_rx = None;
        let mut preset_watcher = None;
        let (presets, preset_note) = if let Some(prfp_path) = discover_latest_prfpset_file() {
            let (tx, rx) = mpsc::channel();
            if let Ok(mut w) = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
                if res.is_ok() {
                    let _ = tx.send(());
                }
            }) {
                let _ = w.watch(&prfp_path, RecursiveMode::NonRecursive);
                preset_reload_rx = Some(rx);
                preset_watcher = Some(w);
            }
            match parse_prfpset_file(&prfp_path) {
                Ok(list) => {
                    let count = list.len();
                    (list, format!("Presets: {count}"))
                }
                Err(_) => (Vec::new(), "Presets: default".into()),
            }
        } else {
            (Vec::new(), "Presets: default".into())
        };

        // Load embedded + disk Flow-generated ARZ Excalibur watermark & live clip-frame preview bank
        let watermark_tex = load_embedded_watermark(&cc.egui_ctx);
        let preview_frame_path = std::env::temp_dir().join("arz_excalibur_live_frame.png");
        let preview_bank = PreviewFrameBank::try_load_from_disk(&cc.egui_ctx, &preview_frame_path, None)
            .unwrap_or_else(|| PreviewFrameBank::fallback(&cc.egui_ctx));
        let mut custom_previews = HashMap::new();
        if let Ok(entries) = std::fs::read_dir("previews") {
            for ent in entries.flatten() {
                let p = ent.path();
                if let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()) {
                    if let Some(tex) = try_load_png_texture(&cc.egui_ctx, &stem, &p) {
                        custom_previews.insert(stem, tex);
                    }
                }
            }
        }

        let (_, fg_title) = detect_foreground_host();
        let user_macros = load_user_macros(Path::new("macros.json"));
        let mut actions = build_premiere_catalog(&presets, &user_macros);
        for act in &mut actions {
            act.entry.enabled = !app_settings.hidden_ids.contains(&act.entry.id);
        }
        if let Ok(mut guard) = bridge.lock() {
            guard.catalog_snapshot = actions.iter().map(|a| a.entry.clone()).collect();
            guard.macros_snapshot = user_macros.clone();
        }

        Self {
            query: String::new(),
            input_primary: String::new(),
            input_secondary: String::new(),
            selected_idx: 0,
            submenu_mode_idx: 0,
            active_submenu: None,
            actions,
            user_macros,
            app_settings: app_settings.clone(),
            bridge,
            hotkey_rx,
            preset_reload_rx,
            _preset_watcher: preset_watcher,
            watermark_tex,
            preview_bank,
            preview_frame_path,
            custom_previews,
            managed_by_premiere: start_hidden,
            visible: !start_hidden,
            status_line: format!(
                "ARZ-Excalibur ({}) | {preset_note} | {fg_title}",
                app_settings.toggle_shortcut
            ),
        }
    }

    fn enqueue_premiere(&mut self, cmd: PremiereCommand, summary: &str) {
        if let Ok(mut guard) = self.bridge.lock() {
            guard.pending_commands.push_back(cmd);
        }
        self.status_line = format!("Applied: {summary}");
    }

    fn hide_and_refocus_premiere(&mut self, ctx: &egui::Context) {
        self.active_submenu = None;
        self.query.clear();
        self.input_primary.clear();
        self.input_secondary.clear();
        self.selected_idx = 0;
        self.visible = false;
        hide_arz_window();
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        let _ = focus_premiere_window();
    }

    fn execute_direct_item(&mut self, item: &ActionItem, ctx: &egui::Context) {
        match &item.payload {
            CommandPayload::UserMacro(m) => {
                execute_user_macro_async(m.clone(), Arc::clone(&self.bridge));
                self.status_line = format!("Macro: {}", m.name);
            }
            CommandPayload::PremiereEffect { display_name, is_video } => {
                self.enqueue_premiere(
                    PremiereCommand::ApplyEffect {
                        name: (*display_name).to_string(),
                        is_video: *is_video,
                    },
                    display_name,
                );
            }
            CommandPayload::PremiereTransition { display_name, is_video } => {
                self.enqueue_premiere(
                    PremiereCommand::ApplyTransition {
                        name: (*display_name).to_string(),
                        is_video: *is_video,
                    },
                    display_name,
                );
            }
            CommandPayload::UserPreset(preset) => {
                self.enqueue_premiere(
                    PremiereCommand::ApplyCompiledPreset {
                        preset: preset.clone(),
                    },
                    &preset.preset_name,
                );
            }
            CommandPayload::TrimToPlayhead { trim_in } => {
                self.enqueue_premiere(
                    PremiereCommand::TrimToPlayhead { trim_in: *trim_in },
                    &item.entry.name,
                );
            }
            CommandPayload::MoveClipToPlayhead { align_start } => {
                self.enqueue_premiere(
                    PremiereCommand::MoveClipToPlayhead { align_start: *align_start },
                    &item.entry.name,
                );
            }
            CommandPayload::SelectRelativeToPlayhead { after } => {
                self.enqueue_premiere(
                    PremiereCommand::SelectClipsRelativeToPlayhead { after: *after },
                    &item.entry.name,
                );
            }
            CommandPayload::ToggleMuteAllTracks { is_video } => {
                self.enqueue_premiere(
                    PremiereCommand::SetTracksState {
                        track_kind: if *is_video { "video".into() } else { "audio".into() },
                        field: "mute".into(),
                        toggle: true,
                    },
                    &item.entry.name,
                );
            }
            CommandPayload::MenuBarClick(path) => match trigger_premiere_menu(path) {
                Ok(()) => self.status_line = format!("Menu: {}", path.join(" > ")),
                Err(e) => self.status_line = e,
            },
            _ => {}
        }
        self.hide_and_refocus_premiere(ctx);
    }

    fn execute_submenu(&mut self, item: &ActionItem, ctx: &egui::Context) {
        match &item.payload {
            CommandPayload::ClipProperty { component, param, dual_axis } => {
                if self.submenu_mode_idx == 1 {
                    self.enqueue_premiere(
                        PremiereCommand::SetClipProperty {
                            component: (*component).into(),
                            param: (*param).into(),
                            op: "toggle_keyframe".into(),
                            value: 0.0,
                            axis: None,
                        },
                        &format!("{param} [Add Keyframe]"),
                    );
                } else {
                    let send_expr = |slf: &mut Self, raw: &str, axis: Option<usize>| {
                        if let Some(expr) = ValueExpr::parse(raw) {
                            let (op, val) = match expr {
                                ValueExpr::Set(v) => ("set", v),
                                ValueExpr::Add(v) => ("add", v),
                                ValueExpr::Sub(v) => ("sub", v),
                                ValueExpr::Mul(v) => ("mul", v),
                                ValueExpr::Div(v) => ("div", v),
                                ValueExpr::Percent(v) => ("percent", v),
                            };
                            slf.enqueue_premiere(
                                PremiereCommand::SetClipProperty {
                                    component: (*component).into(),
                                    param: (*param).into(),
                                    op: op.into(),
                                    value: val,
                                    axis,
                                },
                                &format!("{param} ({op} {val})"),
                            );
                        }
                    };
                    if *dual_axis {
                        send_expr(self, &self.input_primary.clone(), Some(0));
                        send_expr(self, &self.input_secondary.clone(), Some(1));
                    } else {
                        send_expr(self, &self.input_primary.clone(), None);
                    }
                }
            }
            CommandPayload::NestClips => {
                let seq_name = if self.input_primary.trim().is_empty() {
                    "Nested Sequence".into()
                } else {
                    self.input_primary.trim().to_string()
                };
                self.enqueue_premiere(
                    PremiereCommand::NestClips {
                        sequence_name: seq_name.clone(),
                        individual: false,
                    },
                    &format!("Nest -> {seq_name}"),
                );
            }
            CommandPayload::AddSequenceMarker | CommandPayload::AddClipMarker => {
                let on_clip = matches!(item.payload, CommandPayload::AddClipMarker);
                self.enqueue_premiere(
                    PremiereCommand::AddMarker {
                        name: self.input_primary.trim().to_string(),
                        color_index: 0,
                        on_clip,
                    },
                    &item.entry.name,
                );
            }
            CommandPayload::MovePlayhead => {
                if let Ok(delta) = self.input_primary.trim().replace('+', "").parse::<i64>() {
                    self.enqueue_premiere(
                        PremiereCommand::MovePlayhead { delta_frames: delta },
                        &format!("Move Playhead ({delta} frames)"),
                    );
                }
            }
            _ => {}
        }
        self.hide_and_refocus_premiere(ctx);
    }
}

impl eframe::App for ArzExcaliburApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Auto-exit immediately when Premiere Pro closes (`POST /shutdown` or >4s heartbeat loss)
        if let Ok(guard) = self.bridge.lock() {
            if guard.shutdown_requested {
                std::process::exit(0);
            }
            if self.managed_by_premiere {
                if let Some(last_ping) = guard.last_poll_instant {
                    if last_ping.elapsed() > Duration::from_secs(4) {
                        std::process::exit(0);
                    }
                }
            }
        }
        if !self.visible {
            ctx.request_repaint_after(Duration::from_millis(80));
        }

        // Auto-reload `.prfpset` user presets whenever Premiere Pro saves changes
        if let Some(rx) = &self.preset_reload_rx {
            if rx.try_recv().is_ok() {
                while rx.try_recv().is_ok() {}
                if let Some(p) = discover_latest_prfpset_file() {
                    if let Ok(list) = parse_prfpset_file(&p) {
                        self.actions = build_premiere_catalog(&list, &self.user_macros);
                        for act in &mut self.actions {
                            act.entry.enabled = !self.app_settings.hidden_ids.contains(&act.entry.id);
                        }
                        if let Ok(mut guard) = self.bridge.lock() {
                            guard.catalog_snapshot = self.actions.iter().map(|a| a.entry.clone()).collect();
                        }
                        self.status_line = format!("Presets refreshed: {}", list.len());
                    }
                }
            }
        }

        // Poll global Win32 hotkey & shortcut recorder events
        while let Ok(evt) = self.hotkey_rx.try_recv() {
            match evt {
                HotkeyEvent::TogglePalette { premiere_hwnd, window_title } => {
                    self.visible = !self.visible;
                    if self.visible {
                        // Request fresh playhead frame from Premiere Pro for live effect previews!
                        if let Ok(mut guard) = self.bridge.lock() {
                            guard.pending_commands.push_back(PremiereCommand::CapturePreviewFrame {
                                output_path: self.preview_frame_path.to_string_lossy().replace('\\', "/"),
                            });
                        }
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(BAR_W, BAR_EMPTY_H)));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        show_and_center_arz_window(BAR_W as i32, BAR_EMPTY_H as i32);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        if !window_title.is_empty() {
                            self.status_line = format!("Target [0x{premiere_hwnd:x}]: {window_title}");
                        }
                    } else {
                        self.hide_and_refocus_premiere(ctx);
                    }
                }
                HotkeyEvent::DirectShortcut { key_code, ctrl, alt, shift, premiere_hwnd } => {
                    if premiere_hwnd != 0 {
                        // 1. Check per-command shortcuts configured in Premiere Settings (`command_shortcuts`)
                        let mut matched_cmd: Option<ActionItem> = None;
                        for (cmd_id, sc_str) in &self.app_settings.command_shortcuts {
                            if let Some((vk, sc_c, sc_a, sc_s)) = parse_shortcut_str(sc_str) {
                                if vk == key_code && sc_c == ctrl && sc_a == alt && sc_s == shift {
                                    matched_cmd = self.actions.iter().find(|a| &a.entry.id == cmd_id).cloned();
                                    break;
                                }
                            }
                        }
                        if let Some(chosen) = matched_cmd {
                            if chosen.entry.has_submenu {
                                self.visible = true;
                                self.active_submenu = Some(chosen);
                                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                            } else {
                                self.execute_direct_item(&chosen, ctx);
                            }
                        } else {
                            // 2. Fallback: direct shortcut embedded inside `macros.json`
                            for m in &self.user_macros {
                                if m.shortcut_vk == Some(key_code)
                                    && m.ctrl == ctrl
                                    && m.alt == alt
                                    && m.shift == shift
                                {
                                    execute_user_macro_async(m.clone(), Arc::clone(&self.bridge));
                                    self.status_line = format!("Macro: {} (0x{:x})", m.name, last_premiere_hwnd());
                                    break;
                                }
                            }
                        }
                    }
                }
                HotkeyEvent::RecordedToggleShortcut { key_code, ctrl, alt, shift } => {
                    let label = format_shortcut(key_code, ctrl, alt, shift);
                    self.app_settings.toggle_shortcut = label.clone();
                    self.app_settings.vk = key_code;
                    self.app_settings.ctrl = ctrl;
                    self.app_settings.alt = alt;
                    self.app_settings.shift = shift;
                    let _ = save_settings(&self.app_settings);
                    self.active_submenu = None;
                    self.status_line = format!("Shortcut saved: {label}");
                }
            }
        }

        let (reload_cfg, reload_macs) = if let Ok(mut guard) = self.bridge.lock() {
            let rc = guard.reload_settings_flag;
            let rm = guard.reload_macros_flag;
            guard.reload_settings_flag = false;
            guard.reload_macros_flag = false;
            (rc, rm)
        } else {
            (false, false)
        };
        if reload_cfg {
            self.app_settings = load_settings();
            for act in &mut self.actions {
                act.entry.enabled = !self.app_settings.hidden_ids.contains(&act.entry.id);
            }
        }
        if reload_macs {
            self.user_macros = load_user_macros(Path::new("macros.json"));
            let presets = discover_latest_prfpset_file()
                .and_then(|p| parse_prfpset_file(&p).ok())
                .unwrap_or_default();
            self.actions = build_premiere_catalog(&presets, &self.user_macros);
            for act in &mut self.actions {
                act.entry.enabled = !self.app_settings.hidden_ids.contains(&act.entry.id);
            }
            if let Ok(mut guard) = self.bridge.lock() {
                guard.catalog_snapshot = self.actions.iter().map(|a| a.entry.clone()).collect();
            }
        }

        // STRICT BACKGROUND SILENT MODE: when hidden, keep Win32 HWND hidden and skip ALL UI layout/focus!
        if !self.visible {
            hide_arz_window();
            ctx.request_repaint_after(Duration::from_millis(75));
            return;
        }

        let panel_frame = egui::Frame::none()
            .fill(Color32::from_rgb(12, 10, 8)) // Deep Noir Obsidian
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(82, 62, 34))) // Subtle 1px Warm Brass hairline border
            .inner_margin(egui::Margin::symmetric(18.0, 0.0));

        egui::CentralPanel::default().frame(panel_frame).show(ctx, |ui| {
            if let Some(sub_item) = self.active_submenu.clone() {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(480.0, 210.0)));
                ui.horizontal(|ui| {
                    ui.heading(&sub_item.entry.name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("← Back (Esc)").clicked()
                            || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            set_shortcut_recording_mode(false);
                            self.active_submenu = None;
                            self.input_primary.clear();
                            self.input_secondary.clear();
                            self.submenu_mode_idx = 0;
                        }
                    });
                });
                ui.separator();

                match &sub_item.payload {
                    CommandPayload::ChangeOpenShortcut => {
                        ctx.request_repaint_after(Duration::from_millis(50));
                        set_shortcut_recording_mode(true);
                        ui.vertical_centered(|ui| {
                            ui.add_space(12.0);
                            ui.label(format!("Current Shortcut: {}", self.app_settings.toggle_shortcut));
                            ui.add_space(6.0);
                            ui.colored_label(
                                Color32::from_rgb(219, 155, 62),
                                "Press any new key combination now (e.g. Ctrl+Space, Alt+Q, F1)...",
                            );
                            ui.small("Saves automatically to settings.json (Esc to cancel)");
                        });
                    }
                    CommandPayload::ClipProperty { dual_axis, .. } => {
                        let r1 = ui.add(
                            egui::TextEdit::singleline(&mut self.input_primary)
                                .hint_text(if *dual_axis {
                                    "X value (e.g. 960, +50, -20, *1.5, /2)"
                                } else {
                                    "Value or math (+20, -10, *1.5, /2, %50)"
                                })
                                .desired_width(f32::INFINITY),
                        );
                        if self.input_secondary.is_empty() && !r1.has_focus() {
                            r1.request_focus();
                        }
                        if *dual_axis {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.input_secondary)
                                    .hint_text("Y value (e.g. 540, +50, -20)")
                                    .desired_width(f32::INFINITY),
                            );
                        }

                        if let Some(expr) = ValueExpr::parse(&self.input_primary) {
                            let kind = if expr.is_relative_only() { "Relative" } else { "Direct" };
                            ui.small(format!(
                                "{kind} calculator preview (from 100.0) → {:.2}",
                                expr.apply(100.0)
                            ));
                        }

                        ui.add_space(4.0);
                        let modes = ["Apply Value / Math", "Add Keyframe at Playhead"];
                        for (idx, m) in modes.iter().enumerate() {
                            if ui.selectable_label(self.submenu_mode_idx == idx, *m).clicked() {
                                self.submenu_mode_idx = idx;
                                self.execute_submenu(&sub_item, ctx);
                            }
                        }
                        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowUp)) {
                            self.submenu_mode_idx = 1 - self.submenu_mode_idx;
                        }
                        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.execute_submenu(&sub_item, ctx);
                        }
                    }
                    _ => {
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut self.input_primary)
                                .hint_text("Enter name or frame offset (+15, -15) and press Enter...")
                                .desired_width(f32::INFINITY),
                        );
                        r.request_focus();
                        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.execute_submenu(&sub_item, ctx);
                        }
                    }
                }
            } else {
                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.hide_and_refocus_premiere(ctx);
                    return;
                }

                let has_query = !self.query.trim().is_empty();

                // Minimalist 500x84 Bar Mode when empty: dead-center optical watermark + vertically centered input cursor!
                if !has_query {
                    ctx.request_repaint_after(Duration::from_millis(60));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(BAR_W, BAR_EMPTY_H)));

                    let full_rect = ui.max_rect();
                    if let Some(tex) = &self.watermark_tex {
                        let aspect = tex.size_vec2().x / tex.size_vec2().y.max(1.0);
                        let wm_h = 60.0_f32; // Leaves crisp 12px breathing room top & bottom inside 84px bar
                        let wm_w = wm_h * aspect;
                        // Slight -3px optical lift aligns "ARZ Excalibur" text & sword axis dead-center vertically
                        let wm_center = full_rect.center() + Vec2::new(0.0, -3.0);
                        let img_rect = egui::Rect::from_center_size(wm_center, Vec2::new(wm_w, wm_h));
                        ui.painter().image(
                            tex.id(),
                            img_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::from_rgba_unmultiplied(218, 182, 132, 74),
                        );
                    }

                    // Vertically centered single-line input box across the middle of the 84px bar
                    let input_rect = egui::Rect::from_center_size(
                        full_rect.center(),
                        Vec2::new(full_rect.width(), 32.0),
                    );
                    let search_resp = ui.put(
                        input_rect,
                        egui::TextEdit::singleline(&mut self.query)
                            .frame(false)
                            .font(egui::FontId::proportional(22.0))
                            .text_color(Color32::from_rgb(244, 238, 230))
                            .lock_focus(true)
                            .desired_width(f32::INFINITY),
                    );
                    search_resp.request_focus();
                    return;
                }

                ui.add_space(12.0);
                let search_resp = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .frame(false)
                        .font(egui::FontId::proportional(21.0))
                        .text_color(Color32::from_rgb(244, 238, 230))
                        .lock_focus(true)
                        .desired_width(f32::INFINITY),
                );
                search_resp.request_focus();
                ui.add_space(6.0);

                // Hot-reload fresh Premiere playhead frame as soon as `exportFramePNG` writes it!
                if let Some(updated_bank) = PreviewFrameBank::try_load_from_disk(
                    ctx,
                    &self.preview_frame_path,
                    self.preview_bank.last_mtime,
                ) {
                    self.preview_bank = updated_bank;
                }

                // 60-FPS continuous repaint while Grid & Live Effect Preview are visible!
                ctx.request_repaint();

                let mut matched_indices: Vec<(u32, usize)> = self
                    .actions
                    .iter()
                    .enumerate()
                    .filter_map(|(i, act)| act.entry.match_score(&self.query).map(|sc| (sc, i)))
                    .collect();
                matched_indices.sort_by(|a, b| {
                    b.0.cmp(&a.0)
                        .then_with(|| self.actions[a.1].entry.name.cmp(&self.actions[b.1].entry.name))
                });

                let count = matched_indices.len();
                if self.selected_idx >= count {
                    self.selected_idx = 0;
                }

                let cols = if self.app_settings.grid_mode { GRID_COLS } else { 1 };
                // Full 2D Arrow-Key Grid / List Navigation
                if count > 0 {
                    if self.app_settings.grid_mode && ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                        self.selected_idx = (self.selected_idx + 1) % count;
                    }
                    if self.app_settings.grid_mode && ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                        self.selected_idx = self.selected_idx.checked_sub(1).unwrap_or(count - 1);
                    }
                    if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                        self.selected_idx = (self.selected_idx + cols).min(count - 1);
                    }
                    if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                        self.selected_idx = self.selected_idx.saturating_sub(cols);
                    }
                    if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let chosen = self.actions[matched_indices[self.selected_idx].1].clone();
                        if chosen.entry.has_submenu {
                            self.active_submenu = Some(chosen);
                            return;
                        } else {
                            self.execute_direct_item(&chosen, ctx);
                            return;
                        }
                    }
                }

                if !self.app_settings.grid_mode {
                    let win_h = (74.0 + (count.min(9) as f32) * 28.0).clamp(110.0, 340.0);
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(480.0, win_h)));
                    ui.separator();
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for (vis_idx, (_, act_idx)) in matched_indices.iter().enumerate() {
                            let act = &self.actions[*act_idx];
                            let is_sel = vis_idx == self.selected_idx;
                            let resp = ui.selectable_label(is_sel, &act.entry.name);
                            if is_sel {
                                resp.scroll_to_me(Some(egui::Align::Center));
                            }
                            if resp.clicked() {
                                let chosen = act.clone();
                                if chosen.entry.has_submenu {
                                    self.active_submenu = Some(chosen);
                                } else {
                                    self.execute_direct_item(&chosen, ctx);
                                }
                            }
                        }
                    });
                    return;
                }

                let rows = count.div_ceil(GRID_COLS).max(1);
                let visible_rows = rows.min(3) as f32;
                let win_h = (84.0 + visible_rows * 142.0).clamp(160.0, 510.0);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(600.0, win_h)));

                ui.separator();

                let now = ctx.input(|i| i.time);
                let card_w = 180.0_f32;
                let card_h = 132.0_f32;

                egui::ScrollArea::vertical().max_height(410.0).show(ui, |ui| {
                    for row_chunk in matched_indices
                        .iter()
                        .enumerate()
                        .collect::<Vec<_>>()
                        .chunks(GRID_COLS)
                    {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);
                            for &(vis_idx, (_, act_idx)) in row_chunk {
                                let act = &self.actions[*act_idx];
                                let is_sel = vis_idx == self.selected_idx;

                                let (card_rect, resp) = ui.allocate_exact_size(
                                    Vec2::new(card_w, card_h),
                                    egui::Sense::click(),
                                );

                                if is_sel {
                                    resp.scroll_to_me(Some(egui::Align::Center));
                                }

                                let bg_col = if is_sel {
                                    Color32::from_rgb(40, 31, 23) // #281F17 highlighted card
                                } else {
                                    Color32::from_rgb(28, 22, 16) // #1C1610 surface
                                };
                                let border = if is_sel {
                                    Stroke::new(2.0_f32, Color32::from_rgb(219, 155, 62)) // Active #DB9B3E Brass
                                } else {
                                    Stroke::new(1.0_f32, Color32::from_rgb(62, 46, 25))
                                };

                                ui.painter().rect_filled(card_rect, Rounding::same(8.0), bg_col);

                                // Upper 70% of card: Live 60-FPS Animated Preview (or custom PNG in `previews/`)
                                let preview_rect = egui::Rect::from_min_max(
                                    card_rect.min + Vec2::new(8.0, 8.0),
                                    egui::pos2(card_rect.max.x - 8.0, card_rect.min.y + 84.0),
                                );

                                let key_low = act.entry.name.to_lowercase();
                                if let Some(custom_tex) = self.custom_previews.get(&key_low) {
                                    ui.painter().image(
                                        custom_tex.id(),
                                        preview_rect,
                                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                        Color32::WHITE,
                                    );
                                } else {
                                    draw_effect_preview(
                                        ui.painter(),
                                        preview_rect,
                                        act,
                                        &self.preview_bank,
                                        is_sel,
                                        now,
                                    );
                                }

                                // Lower 30% of card: Effect/Command Name + Crisp Uppercase Badge (zero broken boxes!)
                                ui.painter().text(
                                    egui::pos2(card_rect.center().x, card_rect.min.y + 98.0),
                                    egui::Align2::CENTER_CENTER,
                                    &act.entry.name,
                                    egui::FontId::proportional(13.5),
                                    if is_sel {
                                        Color32::from_rgb(244, 238, 230)
                                    } else {
                                        Color32::from_rgb(195, 184, 170)
                                    },
                                );
                                ui.painter().text(
                                    egui::pos2(card_rect.center().x, card_rect.min.y + 116.0),
                                    egui::Align2::CENTER_CENTER,
                                    act.entry.category.badge(),
                                    egui::FontId::proportional(10.0),
                                    if is_sel {
                                        Color32::from_rgb(219, 155, 62)
                                    } else {
                                        Color32::from_rgb(115, 104, 91)
                                    },
                                );

                                ui.painter().rect_stroke(card_rect, Rounding::same(8.0), border);

                                if resp.hovered() {
                                    self.selected_idx = vis_idx;
                                }
                                if resp.clicked() {
                                    let chosen = act.clone();
                                    if chosen.entry.has_submenu {
                                        self.active_submenu = Some(chosen);
                                    } else {
                                        self.execute_direct_item(&chosen, ctx);
                                    }
                                }
                            }
                        });
                        ui.add_space(6.0);
                    }
                });
            }
        });
    }
}

fn load_window_icon() -> egui::IconData {
    let bytes = include_bytes!("../assets/app_icon_64.png");
    if let Ok(im) = image::load_from_memory(bytes) {
        let rgba = im.into_rgba8();
        let (width, height) = rgba.dimensions();
        return egui::IconData {
            rgba: rgba.into_raw(),
            width,
            height,
        };
    }
    egui::IconData::default()
}

fn main() -> eframe::Result<()> {
    // Single-instance check: if another ARZ-Excalibur background daemon is already listening on 127.0.0.1:49872, exit cleanly!
    let Some(bridge) = try_start_loopback_bridge_server() else {
        return Ok(());
    };

    // Start hidden in the background unless `--show` is explicitly requested
    let show_immediately = std::env::args().any(|a| a == "--show");
    let start_hidden = !show_immediately;

    let mut vp = egui::ViewportBuilder::default()
        .with_title("ARZ-Excalibur")
        .with_icon(load_window_icon())
        .with_inner_size([BAR_W, BAR_EMPTY_H])
        .with_decorations(false)
        .with_taskbar(false)
        .with_visible(!start_hidden)
        .with_always_on_top();

    if start_hidden {
        // Park off-screen during initial GL context creation so zero frame-0 flicker ever occurs!
        vp = vp.with_position([-10000.0, -10000.0]);
    }

    let options = eframe::NativeOptions {
        centered: !start_hidden,
        viewport: vp,
        ..Default::default()
    };
    eframe::run_native(
        "ARZ-Excalibur",
        options,
        Box::new(move |cc| Ok(Box::new(ArzExcaliburApp::new(cc, bridge, start_hidden)))),
    )
}

#[cfg(test)]
mod tests {
    use crate::math_expr::ValueExpr;
    use crate::premiere_presets::parse_prfpset_xml;
    use crate::search::{search_commands, CommandCategory, CommandEntry};

    #[test]
    fn test_inline_math_calculator() {
        assert_eq!(ValueExpr::parse("+25").unwrap().apply(100.0), 125.0);
        assert_eq!(ValueExpr::parse("-15").unwrap().apply(100.0), 85.0);
        assert_eq!(ValueExpr::parse("*1.5").unwrap().apply(100.0), 150.0);
        assert_eq!(ValueExpr::parse("/2").unwrap().apply(100.0), 50.0);
        assert_eq!(ValueExpr::parse("%75").unwrap().apply(200.0), 150.0);
        assert_eq!(ValueExpr::parse("140").unwrap().apply(100.0), 140.0);
    }

    #[test]
    fn test_prefix_token_search() {
        let cat = vec![
            CommandEntry { id: "1".into(), name: "Trim In Point to Playhead".into(), category: CommandCategory::Clip, has_submenu: false, enabled: true },
            CommandEntry { id: "2".into(), name: "Cross Dissolve".into(), category: CommandCategory::VideoTransition, has_submenu: false, enabled: true },
        ];
        let hits = search_commands(&cat, "tr in");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "Trim In Point to Playhead");
    }

    #[test]
    fn test_prfpset_xml_compiler() {
        let sample_xml = r#"
        <PremiereData>
          <TreeItem ObjectID="1">
            <TreeItemBase><Name>ARZ Zoom Punch</Name><Data ObjectRef="10"/></TreeItemBase>
          </TreeItem>
          <FilterPresetItem ObjectID="10">
            <EffectItem><EffectFlavor>5</EffectFlavor><EffectIsShorcut>false</EffectIsShorcut></EffectItem>
            <FilterPresets><FilterPreset ObjectRef="20"/></FilterPresets>
          </FilterPresetItem>
          <FilterPreset ObjectID="20">
            <Component ObjectRef="30"/>
            <FilterMatchName>ADBE Geometry2</FilterMatchName>
            <TransitionDuration>0</TransitionDuration>
            <Type>0</Type>
          </FilterPreset>
          <VideoFilterComponent ObjectID="30">
            <MatchName>ADBE Geometry2</MatchName>
            <Component>
              <DisplayName>Transform</DisplayName>
              <Params><Param ObjectRef="40"/></Params>
            </Component>
          </VideoFilterComponent>
          <VideoComponentParam ObjectID="40">
            <Name>Scale</Name>
            <CurrentValue>135.5</CurrentValue>
            <IsTimeVarying>false</IsTimeVarying>
          </VideoComponentParam>
        </PremiereData>"#;

        let presets = parse_prfpset_xml(sample_xml).unwrap();
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].preset_name, "ARZ Zoom Punch");
        assert_eq!(presets[0].effects[0].display_name, "Transform");
        assert_eq!(presets[0].effects[0].params[0].current_value, 135.5);
    }
}
