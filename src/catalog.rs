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
    /// 公式の設定表・スキーマの外 (更新履歴・公開版のソースなど) で確かめた状態の理由。
    #[serde(default)]
    pub status_reason: Option<String>,
    /// 状態の根拠の出典。設定の定義の出典 (`source`) とは別に持つ。
    #[serde(default)]
    pub status_source: Option<String>,
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
            let parts = key_path(key);
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

/// キーの部品が TOML の裸のキーに使える文字 (英数字・`_`・`-`) だけでできているか。
fn is_bare(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// キーの部品が任意名・配列要素のテンプレート (`<id>`・`*`・`<Event>[]`・`hooks[]`) か。
/// 生成器 (scripts/update_catalog.py) の `TEMPLATE_PART` と同じ規則。
pub fn is_template_part(part: &str) -> bool {
    let placeholder = |name: &str| {
        name.strip_prefix('<')
            .and_then(|name| name.strip_suffix('>'))
            .is_some_and(is_bare)
    };
    match part.strip_suffix("[]") {
        Some(element) => element == "*" || is_bare(element) || placeholder(element),
        None => part == "*" || placeholder(part),
    }
}

/// カタログのキーの正規表記を部品に分ける。
///
/// 裸のキーとテンプレートの部品はそのまま、それ以外は JSON の文字列として引用した表記
/// (`notice."hide_gpt-5.1-codex-max_migration_prompt"`) を読む。生成器の `split_key` と
/// 同じ規則で、同梱データが正規表記であることはテストで確かめている。読めない表記は
/// `.` で区切る。
pub fn key_path(key: &str) -> Vec<String> {
    // 引用符のないキーは、読めても読めなくても「.」で区切った結果と同じになる。
    // 一覧を作るたびに全項目で呼ばれるので、検査を省く。
    if !key.contains('"') {
        return key.split('.').map(String::from).collect();
    }
    parse_key(key).unwrap_or_else(|| key.split('.').map(String::from).collect())
}

/// 正規表記として読めたときだけ部品を返す。
pub(crate) fn parse_key(key: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut rest = key;
    loop {
        let after = if rest.starts_with('"') {
            // 閉じる引用符を探す。エスケープの中身の検査は serde_json に任せる。
            let bytes = rest.as_bytes();
            let mut end = 1;
            loop {
                match bytes.get(end)? {
                    b'\\' => end += 2,
                    b'"' => break,
                    _ => end += 1,
                }
            }
            parts.push(serde_json::from_str::<String>(&rest[..=end]).ok()?);
            &rest[end + 1..]
        } else {
            let end = rest.find('.').unwrap_or(rest.len());
            let part = &rest[..end];
            if !is_bare(part) && !is_template_part(part) {
                return None;
            }
            parts.push(part.into());
            &rest[end..]
        };
        if after.is_empty() {
            return Some(parts);
        }
        rest = after.strip_prefix('.').filter(|rest| !rest.is_empty())?;
    }
}

/// 部品を正規表記のキーにする。ドットや引用符を含む名前も1つの部品のまま表す。
pub fn key_text(path: &[String]) -> String {
    let mut text = String::new();
    for (index, part) in path.iter().enumerate() {
        if index > 0 {
            text.push('.');
        }
        if is_bare(part) || is_template_part(part) {
            text.push_str(part);
        } else {
            text.push_str(&serde_json::Value::from(part.as_str()).to_string());
        }
    }
    text
}

pub fn help(provider: Provider, key: &str) -> (String, String) {
    let names = match provider {
        Provider::Claude => &HELP.claude,
        Provider::Codex => &HELP.codex,
    };
    if let Some(text) = names.get(key) {
        return (text.label.clone(), text.description.clone());
    }
    // 引用符のないキーは「.」で区切るだけで部品になり、親のキーも「.」でつなぐだけで
    // 正規表記になる。引用された部品を含むキーだけ、分割と整形の規則を通す。
    let quoted = key.contains('"').then(|| key_path(key));
    let parts: Vec<&str> = match &quoted {
        Some(parts) => parts.iter().map(String::as_str).collect(),
        None => key.split('.').collect(),
    };
    let mut labels = Vec::new();
    let mut start = 0;
    // 名前には最も具体的な既知の親を使う。子の動作が同じとは限らないため、
    // 親の説明は流用しない。
    for length in (1..parts.len()).rev() {
        let parent = match &quoted {
            Some(parts) => key_text(&parts[..length]),
            None => parts[..length].join("."),
        };
        if let Some(text) = names.get(&parent) {
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
