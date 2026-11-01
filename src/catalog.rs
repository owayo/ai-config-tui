use std::sync::LazyLock;

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
}

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
