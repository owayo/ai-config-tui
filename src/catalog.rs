use std::{collections::HashMap, sync::LazyLock};

use serde::Deserialize;
use serde_json::Value;

use crate::{fields::Kind, profile::Provider};

#[derive(Deserialize)]
pub struct Catalog {
    pub verified_at: String,
    pub claude: Vec<Entry>,
    pub codex: Vec<Entry>,
}

#[derive(Deserialize)]
pub struct Entry {
    pub key: String,
    pub type_name: String,
    pub kind: Kind,
    pub choices: Vec<Value>,
    pub custom: bool,
    pub scope: String,
    pub source: String,
    pub default: String,
    pub read_only: Option<String>,
    pub initial: String,
    #[serde(default)]
    pub retired: bool,
    #[serde(default)]
    pub deprecated: bool,
}

#[derive(Deserialize)]
struct SettingHelp {
    label: String,
    description: String,
}

#[derive(Deserialize)]
struct HelpCatalog {
    claude: HashMap<String, SettingHelp>,
    codex: HashMap<String, SettingHelp>,
    segments: HashMap<String, String>,
}

static HELP: LazyLock<HelpCatalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/settings-help.ja.json"))
        .expect("the bundled setting names must be valid")
});

pub static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/settings-catalog.json"))
        .expect("the bundled settings catalog must be valid")
});

pub fn entries(provider: Provider) -> &'static [Entry] {
    match provider {
        Provider::Claude => &CATALOG.claude,
        Provider::Codex => &CATALOG.codex,
    }
}

pub struct ChoiceHelp {
    pub description: String,
    pub source: String,
    pub verified_at: String,
}

impl ChoiceHelp {
    pub fn local(description: &str) -> Self {
        Self {
            description: description.into(),
            source: String::new(),
            verified_at: String::new(),
        }
    }
}

#[derive(Deserialize)]
struct ChoiceText {
    value: Value,
    description: String,
}

#[derive(Deserialize)]
struct ChoiceGroup {
    keys: Vec<String>,
    source: String,
    choices: Vec<ChoiceText>,
}

#[derive(Deserialize)]
struct ChoiceCatalog {
    verified_at: String,
    claude: Vec<ChoiceGroup>,
    codex: Vec<ChoiceGroup>,
}

static CHOICE_HELP: LazyLock<ChoiceCatalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../data/choice-help.ja.json"))
        .expect("the bundled choice explanations must be valid")
});

pub fn choice_help(provider: Provider, path: &[String], value: &Value) -> Option<ChoiceHelp> {
    let groups = match provider {
        Provider::Claude => &CHOICE_HELP.claude,
        Provider::Codex => &CHOICE_HELP.codex,
    };
    for group in groups {
        if !group.keys.iter().any(|key| {
            let parts = key.split('.').collect::<Vec<_>>();
            parts.len() == path.len()
                && parts
                    .iter()
                    .zip(path)
                    .all(|(part, actual)| segment_matches(part, actual))
        }) {
            continue;
        }
        if let Some(text) = group.choices.iter().find(|text| &text.value == value) {
            return Some(ChoiceHelp {
                description: text.description.clone(),
                source: group.source.clone(),
                verified_at: CHOICE_HELP.verified_at.clone(),
            });
        }
    }
    None
}

pub fn segment_matches(pattern: &str, actual: &str) -> bool {
    pattern == actual
        || pattern == "*"
        || (pattern.starts_with('<') && pattern.ends_with('>') && pattern != "<index>")
}

pub fn help(provider: Provider, key: &str) -> (String, String) {
    let names = match provider {
        Provider::Claude => &HELP.claude,
        Provider::Codex => &HELP.codex,
    };
    if let Some(text) = names.get(key) {
        return (text.label.clone(), text.description.clone());
    }
    let parts = key.split('.').collect::<Vec<_>>();
    let mut labels = Vec::new();
    let mut start = 0;
    // 名前には最も具体的な既知の親を使う。子の動作が同じとは限らないため、
    // 親の説明は流用しない。
    for length in (1..parts.len()).rev() {
        if let Some(text) = names.get(&parts[..length].join(".")) {
            labels.push(text.label.clone());
            start = length;
            break;
        }
    }
    for part in &parts[start..] {
        labels.push(
            HELP.segments
                .get(*part)
                .cloned()
                .unwrap_or_else(|| readable_name(part)),
        );
    }
    (labels.join(" / "), String::new())
}

fn readable_name(key: &str) -> String {
    let mut name = String::new();
    let chars = key.chars().collect::<Vec<_>>();
    for (index, &character) in chars.iter().enumerate() {
        if matches!(character, '_' | '-') {
            name.push(' ');
            continue;
        }
        if character.is_ascii_uppercase()
            && index > 0
            && (chars[index - 1].is_ascii_lowercase()
                || (chars[index - 1].is_ascii_uppercase()
                    && chars.get(index + 1).is_some_and(char::is_ascii_lowercase)))
        {
            name.push(' ');
        }
        if name.is_empty() || name.ends_with(' ') {
            name.push(character.to_ascii_uppercase());
        } else {
            name.push(character);
        }
    }
    name
}
