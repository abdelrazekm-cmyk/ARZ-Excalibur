//! Prefix-token and substring command search engine across all editing categories.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandCategory {
    UserMacro,
    Clip,
    Timeline,
    Selection,
    VideoPreset,
    AudioPreset,
    VideoEffect,
    AudioEffect,
    VideoTransition,
    AudioTransition,
    Export,
    Project,
    Preferences,
}

impl CommandCategory {
    pub fn badge(self) -> &'static str {
        match self {
            Self::UserMacro => "USER MACRO",
            Self::Clip => "CLIP",
            Self::Timeline => "TIMELINE",
            Self::Selection => "SELECTION",
            Self::VideoPreset => "VIDEO PRESET",
            Self::AudioPreset => "AUDIO PRESET",
            Self::VideoEffect => "VIDEO EFFECT",
            Self::AudioEffect => "AUDIO EFFECT",
            Self::VideoTransition => "VIDEO TRANSITION",
            Self::AudioTransition => "AUDIO TRANSITION",
            Self::Export => "EXPORT",
            Self::Project => "PROJECT",
            Self::Preferences => "SETTINGS",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEntry {
    pub id: String,
    pub name: String,
    pub category: CommandCategory,
    pub has_submenu: bool,
    pub enabled: bool,
}

impl CommandEntry {
    /// Matches multi-token prefix search (e.g. `"tr in"` -> `"Trim In Point to Playhead"`)
    /// as well as case-insensitive substring match, returning a relevance score (`Some(score)`).
    pub fn match_score(&self, query: &str) -> Option<u32> {
        if !self.enabled {
            return None;
        }
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Some(1);
        }

        let name_lower = self.name.to_lowercase();
        if name_lower == q {
            return Some(1000);
        }
        if name_lower.starts_with(&q) {
            return Some(800);
        }

        // Prefix-token matching (same UX speed as Excalibur's `^tok1\S*\s+?tok2\S*` regex, without regex compilation overhead)
        let q_tokens: Vec<&str> = q.split_whitespace().collect();
        let name_words: Vec<&str> = name_lower.split_whitespace().collect();
        if q_tokens.len() <= name_words.len() {
            let mut word_idx = 0usize;
            let mut all_matched = true;
            let mut anchored_first = false;
            for (ti, qt) in q_tokens.iter().enumerate() {
                let mut found = false;
                while word_idx < name_words.len() {
                    if name_words[word_idx].starts_with(qt) {
                        if ti == 0 && word_idx == 0 {
                            anchored_first = true;
                        }
                        word_idx += 1;
                        found = true;
                        break;
                    }
                    word_idx += 1;
                }
                if !found {
                    all_matched = false;
                    break;
                }
            }
            if all_matched {
                return Some(if anchored_first { 650 } else { 500 });
            }
        }

        if name_lower.contains(&q) {
            return Some(300);
        }

        None
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn search_commands<'a>(catalog: &'a [CommandEntry], query: &str) -> Vec<&'a CommandEntry> {
    let mut scored: Vec<(u32, &'a CommandEntry)> = catalog
        .iter()
        .filter_map(|cmd| cmd.match_score(query).map(|score| (score, cmd)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    scored.into_iter().map(|(_, cmd)| cmd).collect()
}
