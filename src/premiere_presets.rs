//! Fast streaming `.prfpset` XML Effect Preset parser & Premiere Profile Auto-Discovery.
//! Uses `quick-xml` to parse multi-megabyte Premiere Pro `Effect Presets and Custom Items.prfpset`
//! files in a few milliseconds (replaces `client/lib/presets.js` + `xml-flow`).

use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetParamRecipe {
    pub name: String,
    pub current_value: f64,
    pub is_time_varying: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetEffectRecipe {
    pub display_name: String,
    pub match_name: String,
    pub is_video: bool,
    pub anchor_type: i32,
    pub params: Vec<PresetParamRecipe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledPremierePreset {
    pub preset_name: String,
    pub is_video: bool,
    pub effects: Vec<PresetEffectRecipe>,
}

/// Locates the newest `Effect Presets and Custom Items.prfpset` inside the user's
/// `Documents\Adobe\Premiere Pro\<version>\Profile-*\` directory.
pub fn discover_latest_prfpset_file() -> Option<PathBuf> {
    let user_profile = std::env::var_os("USERPROFILE")?;
    let adobe_ppro = Path::new(&user_profile)
        .join("Documents")
        .join("Adobe")
        .join("Premiere Pro");
    if !adobe_ppro.is_dir() {
        return None;
    }

    let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    if let Ok(versions) = fs::read_dir(&adobe_ppro) {
        for ver_entry in versions.flatten() {
            let ver_path = ver_entry.path();
            if !ver_path.is_dir() {
                continue;
            }
            if let Ok(children) = fs::read_dir(&ver_path) {
                for child in children.flatten() {
                    let cpath = child.path();
                    let cname = child.file_name().to_string_lossy().to_lowercase();
                    if cpath.is_dir() && cname.starts_with("profile-") {
                        let prfpset = cpath.join("Effect Presets and Custom Items.prfpset");
                        if let Ok(meta) = fs::metadata(&prfpset) {
                            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                            candidates.push((mtime, prfpset));
                        }
                    }
                }
            }
        }
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.into_iter().map(|(_, p)| p).next()
}

/// Parses a Premiere Pro `.prfpset` XML file into executable `CompiledPremierePreset` recipes.
pub fn parse_prfpset_file(path: &Path) -> Result<Vec<CompiledPremierePreset>, String> {
    let xml = fs::read_to_string(path).map_err(|e| format!("Failed reading .prfpset: {e}"))?;
    parse_prfpset_xml(&xml)
}

pub fn parse_prfpset_xml(xml: &str) -> Result<Vec<CompiledPremierePreset>, String> {
    #[derive(Default)]
    struct RawTreeItem {
        name: String,
        data_ref: String,
    }
    #[derive(Default)]
    struct RawPresetItem {
        flavor: i32,
        is_shortcut: bool,
        preset_refs: Vec<String>,
    }
    #[derive(Default)]
    struct RawFilterPreset {
        component_ref: String,
        filter_match_name: String,
        transition_duration: f64,
        anchor_type: i32,
    }
    #[derive(Default)]
    struct RawComponent {
        display_name: String,
        match_name: String,
        param_refs: Vec<String>,
    }

    let mut tree_items: Vec<RawTreeItem> = Vec::new();
    let mut preset_items: HashMap<String, RawPresetItem> = HashMap::new();
    let mut filter_presets: HashMap<String, RawFilterPreset> = HashMap::new();
    let mut video_comps: HashMap<String, RawComponent> = HashMap::new();
    let mut audio_comps: HashMap<String, RawComponent> = HashMap::new();
    let mut params_map: HashMap<String, PresetParamRecipe> = HashMap::new();

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut stack: Vec<(String, String)> = Vec::new(); // (tag_name, ObjectID_or_ObjectRef)
    let mut current_text = String::new();

    // Scratch state for active containers
    let mut cur_tree_name = String::new();
    let mut cur_tree_data = String::new();
    let mut cur_pitem = RawPresetItem::default();
    let mut cur_fpreset = RawFilterPreset::default();
    let mut cur_comp = RawComponent::default();
    let mut cur_param = PresetParamRecipe {
        name: String::new(),
        current_value: 0.0,
        is_time_varying: false,
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut obj_id = String::new();
                let mut obj_ref = String::new();
                for attr in e.attributes().flatten() {
                    let k = String::from_utf8_lossy(attr.key.as_ref());
                    let v = String::from_utf8_lossy(&attr.value);
                    if k == "ObjectID" {
                        obj_id = v.to_string();
                    } else if k == "ObjectRef" {
                        obj_ref = v.to_string();
                    }
                }

                match tag.as_str() {
                    "TreeItem" => {
                        cur_tree_name.clear();
                        cur_tree_data.clear();
                    }
                    "FilterPresetItem" => cur_pitem = RawPresetItem::default(),
                    "FilterPreset" => {
                        if !obj_ref.is_empty()
                            && stack.iter().any(|(t, _)| t == "FilterPresetItem")
                        {
                            cur_pitem.preset_refs.push(obj_ref.clone());
                        } else {
                            cur_fpreset = RawFilterPreset::default();
                        }
                    }
                    "VideoFilterComponent" | "AudioFilterComponent" => {
                        cur_comp = RawComponent::default();
                    }
                    "Param" => {
                        if !obj_ref.is_empty() {
                            cur_comp.param_refs.push(obj_ref.clone());
                        }
                    }
                    "VideoComponentParam"
                    | "PointComponentParam"
                    | "ArbVideoComponentParam"
                    | "AudioComponentParam" => {
                        cur_param = PresetParamRecipe {
                            name: String::new(),
                            current_value: 0.0,
                            is_time_varying: false,
                        };
                    }
                    _ => {}
                }

                let key_id = if !obj_id.is_empty() { obj_id } else { obj_ref };
                stack.push((tag, key_id));
                current_text.clear();
            }
            Ok(Event::Empty(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut obj_ref = String::new();
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"ObjectRef" {
                        obj_ref = String::from_utf8_lossy(&attr.value).to_string();
                    }
                }
                if tag == "FilterPreset"
                    && !obj_ref.is_empty()
                    && stack.iter().any(|(t, _)| t == "FilterPresetItem")
                {
                    cur_pitem.preset_refs.push(obj_ref);
                } else if tag == "Param" && !obj_ref.is_empty() {
                    cur_comp.param_refs.push(obj_ref);
                } else if tag == "Data" && !obj_ref.is_empty() {
                    if stack.iter().any(|(t, _)| t == "TreeItem") {
                        cur_tree_data = obj_ref;
                    }
                } else if tag == "Component" && !obj_ref.is_empty() {
                    if stack.iter().any(|(t, _)| t == "FilterPreset") {
                        cur_fpreset.component_ref = obj_ref;
                    }
                }
            }
            Ok(Event::Text(t)) => {
                current_text = t.unescape().map(|c| c.to_string()).unwrap_or_default();
            }
            Ok(Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let (_, popped_id) = stack.pop().unwrap_or_default();
                let parent_ctx: Vec<&str> = stack.iter().map(|(t, _)| t.as_str()).collect();

                match tag.as_str() {
                    "Name" => {
                        if parent_ctx.contains(&"TreeItemBase") && parent_ctx.contains(&"TreeItem")
                        {
                            cur_tree_name = current_text.clone();
                        } else if parent_ctx.iter().any(|p| p.ends_with("ComponentParam")) {
                            cur_param.name = current_text.clone();
                        }
                    }
                    "Data" if parent_ctx.contains(&"TreeItemBase") && !popped_id.is_empty() => {
                        cur_tree_data = popped_id;
                    }
                    "EffectFlavor" if parent_ctx.contains(&"FilterPresetItem") => {
                        cur_pitem.flavor = current_text.parse::<i32>().unwrap_or(0);
                    }
                    "EffectIsShorcut" if parent_ctx.contains(&"FilterPresetItem") => {
                        cur_pitem.is_shortcut = current_text == "true";
                    }
                    "Component" if parent_ctx.contains(&"FilterPreset") && !popped_id.is_empty() => {
                        cur_fpreset.component_ref = popped_id;
                    }
                    "FilterMatchName" => {
                        if parent_ctx.contains(&"FilterPreset") {
                            cur_fpreset.filter_match_name = current_text.clone();
                        } else if parent_ctx.contains(&"AudioFilterComponent") {
                            cur_comp.match_name = current_text.clone();
                        }
                    }
                    "TransitionDuration" if parent_ctx.contains(&"FilterPreset") => {
                        cur_fpreset.transition_duration =
                            current_text.parse::<f64>().unwrap_or(0.0);
                    }
                    "Type" if parent_ctx.contains(&"FilterPreset") => {
                        cur_fpreset.anchor_type = current_text.parse::<i32>().unwrap_or(0);
                    }
                    "DisplayName"
                        if parent_ctx.contains(&"VideoFilterComponent")
                            || parent_ctx.contains(&"AudioFilterComponent") =>
                    {
                        cur_comp.display_name = current_text.clone();
                    }
                    "MatchName" if parent_ctx.contains(&"VideoFilterComponent") => {
                        cur_comp.match_name = current_text.clone();
                    }
                    "CurrentValue" if parent_ctx.iter().any(|p| p.ends_with("ComponentParam")) => {
                        cur_param.current_value = current_text.parse::<f64>().unwrap_or(0.0);
                    }
                    "IsTimeVarying" if parent_ctx.iter().any(|p| p.ends_with("ComponentParam")) => {
                        cur_param.is_time_varying = current_text == "true";
                    }
                    "TreeItem" => {
                        if !cur_tree_name.is_empty() && !cur_tree_data.is_empty() {
                            tree_items.push(RawTreeItem {
                                name: cur_tree_name.clone(),
                                data_ref: cur_tree_data.clone(),
                            });
                        }
                    }
                    "FilterPresetItem" if !popped_id.is_empty() => {
                        preset_items.insert(popped_id, std::mem::take(&mut cur_pitem));
                    }
                    "FilterPreset"
                        if !popped_id.is_empty()
                            && !parent_ctx.contains(&"FilterPresetItem") =>
                    {
                        filter_presets.insert(popped_id, std::mem::take(&mut cur_fpreset));
                    }
                    "VideoFilterComponent" if !popped_id.is_empty() => {
                        video_comps.insert(popped_id, std::mem::take(&mut cur_comp));
                    }
                    "AudioFilterComponent" if !popped_id.is_empty() => {
                        audio_comps.insert(popped_id, std::mem::take(&mut cur_comp));
                    }
                    "VideoComponentParam"
                    | "PointComponentParam"
                    | "ArbVideoComponentParam"
                    | "AudioComponentParam"
                        if !popped_id.is_empty() =>
                    {
                        params_map.insert(popped_id, cur_param.clone());
                    }
                    _ => {}
                }
                current_text.clear();
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(format!("XML parse error: {err}")),
            _ => {}
        }
    }

    let mut compiled = Vec::new();
    for ti in tree_items {
        let Some(pitem) = preset_items.get(&ti.data_ref) else {
            continue;
        };
        if pitem.is_shortcut {
            continue;
        }
        let is_video = pitem.flavor != 0;
        let mut effects = Vec::new();
        let mut skip_transition = false;

        for pref in &pitem.preset_refs {
            let Some(fp) = filter_presets.get(pref) else {
                continue;
            };
            if fp.transition_duration > 0.0 {
                skip_transition = true;
                break;
            }
            let comp_opt = if is_video {
                video_comps.get(&fp.component_ref)
            } else {
                audio_comps.get(&fp.component_ref)
            };
            if let Some(comp) = comp_opt {
                let params = comp
                    .param_refs
                    .iter()
                    .filter_map(|pr| params_map.get(pr).cloned())
                    .collect();
                effects.push(PresetEffectRecipe {
                    display_name: comp.display_name.clone(),
                    match_name: if !comp.match_name.is_empty() {
                        comp.match_name.clone()
                    } else {
                        fp.filter_match_name.clone()
                    },
                    is_video,
                    anchor_type: fp.anchor_type,
                    params,
                });
            }
        }

        if !skip_transition && !effects.is_empty() {
            compiled.push(CompiledPremierePreset {
                preset_name: ti.name,
                is_video,
                effects,
            });
        }
    }

    Ok(compiled)
}
