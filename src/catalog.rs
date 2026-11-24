//! Built-in Premiere Pro Command Catalog + Dynamic `.prfpset` User Presets + User Macros.

use crate::macro_engine::UserMacro;
use crate::premiere_presets::CompiledPremierePreset;
use crate::search::{CommandCategory, CommandEntry};

#[derive(Debug, Clone)]
pub enum CommandPayload {
    ChangeOpenShortcut,
    UserMacro(UserMacro),
    ClipProperty {
        component: &'static str,
        param: &'static str,
        dual_axis: bool,
    },
    PremiereEffect {
        display_name: &'static str,
        is_video: bool,
    },
    PremiereTransition {
        display_name: &'static str,
        is_video: bool,
    },
    UserPreset(CompiledPremierePreset),
    TrimToPlayhead {
        trim_in: bool,
    },
    MoveClipToPlayhead {
        align_start: bool,
    },
    NestClips,
    AddSequenceMarker,
    AddClipMarker,
    MovePlayhead,
    SelectRelativeToPlayhead {
        after: bool,
    },
    ToggleMuteAllTracks {
        is_video: bool,
    },
    MenuBarClick(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct ActionItem {
    pub entry: CommandEntry,
    pub payload: CommandPayload,
}

pub fn build_premiere_catalog(
    user_presets: &[CompiledPremierePreset],
    user_macros: &[UserMacro],
) -> Vec<ActionItem> {
    let mut items = Vec::new();
    for m in user_macros {
        items.push(ActionItem {
            entry: CommandEntry {
                id: m.id.clone(),
                name: m.name.clone(),
                category: CommandCategory::UserMacro,
                has_submenu: false,
                enabled: true,
            },
            payload: CommandPayload::UserMacro(m.clone()),
        });
    }
    items.extend(vec![
        ActionItem {
            entry: CommandEntry { id: "pref.shortcut".into(), name: "Change Open Shortcut (تغيير اختصار الفتح)".into(), category: CommandCategory::Preferences, has_submenu: true, enabled: true },
            payload: CommandPayload::ChangeOpenShortcut,
        },
        // --- Clip Transform & Audio Properties (with Inline Relative Calculator + Keyframe) ---
        ActionItem {
            entry: CommandEntry { id: "cl.scale".into(), name: "Scale".into(), category: CommandCategory::Clip, has_submenu: true, enabled: true },
            payload: CommandPayload::ClipProperty { component: "Motion", param: "Scale", dual_axis: false },
        },
        ActionItem {
            entry: CommandEntry { id: "cl.position".into(), name: "Position".into(), category: CommandCategory::Clip, has_submenu: true, enabled: true },
            payload: CommandPayload::ClipProperty { component: "Motion", param: "Position", dual_axis: true },
        },
        ActionItem {
            entry: CommandEntry { id: "cl.rotation".into(), name: "Rotation".into(), category: CommandCategory::Clip, has_submenu: true, enabled: true },
            payload: CommandPayload::ClipProperty { component: "Motion", param: "Rotation", dual_axis: false },
        },
        ActionItem {
            entry: CommandEntry { id: "cl.anchor".into(), name: "Anchor Point".into(), category: CommandCategory::Clip, has_submenu: true, enabled: true },
            payload: CommandPayload::ClipProperty { component: "Motion", param: "Anchor Point", dual_axis: true },
        },
        ActionItem {
            entry: CommandEntry { id: "cl.opacity".into(), name: "Opacity".into(), category: CommandCategory::Clip, has_submenu: true, enabled: true },
            payload: CommandPayload::ClipProperty { component: "Opacity", param: "Opacity", dual_axis: false },
        },
        ActionItem {
            entry: CommandEntry { id: "cl.volume".into(), name: "Volume (Level dB)".into(), category: CommandCategory::Clip, has_submenu: true, enabled: true },
            payload: CommandPayload::ClipProperty { component: "Volume", param: "Level", dual_axis: false },
        },

        // --- Timeline & Clip Editing Operations ---
        ActionItem {
            entry: CommandEntry { id: "cl.nest".into(), name: "Nest Clips".into(), category: CommandCategory::Clip, has_submenu: true, enabled: true },
            payload: CommandPayload::NestClips,
        },
        ActionItem {
            entry: CommandEntry { id: "cl.trim_in".into(), name: "Trim In Point to Playhead".into(), category: CommandCategory::Clip, has_submenu: false, enabled: true },
            payload: CommandPayload::TrimToPlayhead { trim_in: true },
        },
        ActionItem {
            entry: CommandEntry { id: "cl.trim_out".into(), name: "Trim Out Point to Playhead".into(), category: CommandCategory::Clip, has_submenu: false, enabled: true },
            payload: CommandPayload::TrimToPlayhead { trim_in: false },
        },
        ActionItem {
            entry: CommandEntry { id: "cl.move_start".into(), name: "Move Clip Start to Playhead".into(), category: CommandCategory::Clip, has_submenu: false, enabled: true },
            payload: CommandPayload::MoveClipToPlayhead { align_start: true },
        },
        ActionItem {
            entry: CommandEntry { id: "cl.move_end".into(), name: "Move Clip End to Playhead".into(), category: CommandCategory::Clip, has_submenu: false, enabled: true },
            payload: CommandPayload::MoveClipToPlayhead { align_start: false },
        },
        ActionItem {
            entry: CommandEntry { id: "sq.marker_seq".into(), name: "Add Marker to Sequence".into(), category: CommandCategory::Timeline, has_submenu: true, enabled: true },
            payload: CommandPayload::AddSequenceMarker,
        },
        ActionItem {
            entry: CommandEntry { id: "sq.marker_clip".into(), name: "Add Marker to Clip".into(), category: CommandCategory::Timeline, has_submenu: true, enabled: true },
            payload: CommandPayload::AddClipMarker,
        },
        ActionItem {
            entry: CommandEntry { id: "sq.move_playhead".into(), name: "Move Playhead (Frames +/-)".into(), category: CommandCategory::Timeline, has_submenu: true, enabled: true },
            payload: CommandPayload::MovePlayhead,
        },
        ActionItem {
            entry: CommandEntry { id: "sl.after".into(), name: "Select All Clips after Playhead".into(), category: CommandCategory::Selection, has_submenu: false, enabled: true },
            payload: CommandPayload::SelectRelativeToPlayhead { after: true },
        },
        ActionItem {
            entry: CommandEntry { id: "sl.before".into(), name: "Select All Clips before Playhead".into(), category: CommandCategory::Selection, has_submenu: false, enabled: true },
            payload: CommandPayload::SelectRelativeToPlayhead { after: false },
        },
        ActionItem {
            entry: CommandEntry { id: "sq.mute_v".into(), name: "Toggle Mute All Video Tracks".into(), category: CommandCategory::Timeline, has_submenu: false, enabled: true },
            payload: CommandPayload::ToggleMuteAllTracks { is_video: true },
        },
        ActionItem {
            entry: CommandEntry { id: "sq.mute_a".into(), name: "Toggle Mute All Audio Tracks".into(), category: CommandCategory::Timeline, has_submenu: false, enabled: true },
            payload: CommandPayload::ToggleMuteAllTracks { is_video: false },
        },

        // --- Direct Win32 Menu-Bar Actions (No Shortcut Required in Premiere!) ---
        ActionItem {
            entry: CommandEntry { id: "mn.scale_frame".into(), name: "Scale to Frame Size".into(), category: CommandCategory::Clip, has_submenu: false, enabled: true },
            payload: CommandPayload::MenuBarClick(vec!["Clip".into(), "Video Options".into(), "Scale to Frame Size".into()]),
        },
        ActionItem {
            entry: CommandEntry { id: "mn.set_frame".into(), name: "Set to Frame Size".into(), category: CommandCategory::Clip, has_submenu: false, enabled: true },
            payload: CommandPayload::MenuBarClick(vec!["Clip".into(), "Video Options".into(), "Set to Frame Size".into()]),
        },
        ActionItem {
            entry: CommandEntry { id: "mn.frame_hold".into(), name: "Add Frame Hold".into(), category: CommandCategory::Clip, has_submenu: false, enabled: true },
            payload: CommandPayload::MenuBarClick(vec!["Clip".into(), "Video Options".into(), "Add Frame Hold".into()]),
        },
        ActionItem {
            entry: CommandEntry { id: "mn.scene_detect".into(), name: "Scene Edit Detection".into(), category: CommandCategory::Clip, has_submenu: false, enabled: true },
            payload: CommandPayload::MenuBarClick(vec!["Clip".into(), "Scene Edit Detection".into()]),
        },
        ActionItem {
            entry: CommandEntry { id: "mn.audio_gain".into(), name: "Audio Gain".into(), category: CommandCategory::Clip, has_submenu: false, enabled: true },
            payload: CommandPayload::MenuBarClick(vec!["Clip".into(), "Audio Options".into(), "Audio Gain".into()]),
        },
        ActionItem {
            entry: CommandEntry { id: "mn.render_in_out".into(), name: "Render In to Out".into(), category: CommandCategory::Timeline, has_submenu: false, enabled: true },
            payload: CommandPayload::MenuBarClick(vec!["Sequence".into(), "Render In to Out".into()]),
        },

        // --- Core Premiere Pro Video & Audio Effects ---
        ActionItem {
            entry: CommandEntry { id: "vf.transform".into(), name: "Transform".into(), category: CommandCategory::VideoEffect, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereEffect { display_name: "Transform", is_video: true },
        },
        ActionItem {
            entry: CommandEntry { id: "vf.gblur".into(), name: "Gaussian Blur".into(), category: CommandCategory::VideoEffect, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereEffect { display_name: "Gaussian Blur", is_video: true },
        },
        ActionItem {
            entry: CommandEntry { id: "vf.warp".into(), name: "Warp Stabilizer".into(), category: CommandCategory::VideoEffect, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereEffect { display_name: "Warp Stabilizer", is_video: true },
        },
        ActionItem {
            entry: CommandEntry { id: "vf.crop".into(), name: "Crop".into(), category: CommandCategory::VideoEffect, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereEffect { display_name: "Crop", is_video: true },
        },
        ActionItem {
            entry: CommandEntry { id: "vf.lumetri".into(), name: "Lumetri Color".into(), category: CommandCategory::VideoEffect, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereEffect { display_name: "Lumetri Color", is_video: true },
        },
        ActionItem {
            entry: CommandEntry { id: "vf.dropshadow".into(), name: "Drop Shadow".into(), category: CommandCategory::VideoEffect, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereEffect { display_name: "Drop Shadow", is_video: true },
        },
        ActionItem {
            entry: CommandEntry { id: "vf.ultrakey".into(), name: "Ultra Key".into(), category: CommandCategory::VideoEffect, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereEffect { display_name: "Ultra Key", is_video: true },
        },
        ActionItem {
            entry: CommandEntry { id: "af.denoise".into(), name: "DeNoise".into(), category: CommandCategory::AudioEffect, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereEffect { display_name: "DeNoise", is_video: false },
        },
        ActionItem {
            entry: CommandEntry { id: "af.hardlimiter".into(), name: "Hard Limiter".into(), category: CommandCategory::AudioEffect, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereEffect { display_name: "Hard Limiter", is_video: false },
        },

        // --- Core Premiere Pro Transitions ---
        ActionItem {
            entry: CommandEntry { id: "vt.cross_dissolve".into(), name: "Cross Dissolve".into(), category: CommandCategory::VideoTransition, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereTransition { display_name: "Cross Dissolve", is_video: true },
        },
        ActionItem {
            entry: CommandEntry { id: "vt.dip_black".into(), name: "Dip to Black".into(), category: CommandCategory::VideoTransition, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereTransition { display_name: "Dip to Black", is_video: true },
        },
        ActionItem {
            entry: CommandEntry { id: "vt.dip_white".into(), name: "Dip to White".into(), category: CommandCategory::VideoTransition, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereTransition { display_name: "Dip to White", is_video: true },
        },
        ActionItem {
            entry: CommandEntry { id: "vt.film_dissolve".into(), name: "Film Dissolve".into(), category: CommandCategory::VideoTransition, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereTransition { display_name: "Film Dissolve", is_video: true },
        },
        ActionItem {
            entry: CommandEntry { id: "at.const_power".into(), name: "Constant Power".into(), category: CommandCategory::AudioTransition, has_submenu: false, enabled: true },
            payload: CommandPayload::PremiereTransition { display_name: "Constant Power", is_video: false },
        },
    ]);

    // Append all automatically parsed user `.prfpset` presets!
    for (idx, preset) in user_presets.iter().enumerate() {
        items.push(ActionItem {
            entry: CommandEntry {
                id: format!("preset.{idx}"),
                name: preset.preset_name.clone(),
                category: if preset.is_video {
                    CommandCategory::VideoPreset
                } else {
                    CommandCategory::AudioPreset
                },
                has_submenu: false,
                enabled: true,
            },
            payload: CommandPayload::UserPreset(preset.clone()),
        });
    }

    items
}
