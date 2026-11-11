use std::path::PathBuf;

use anyhow::{Result, bail, ensure};
use serde_json::Value as Json;
use toml_edit::{DocumentMut, Item, TableLike, Value as Toml};

use crate::{profile::Provider, storage::Snapshot};

#[derive(Clone, Debug)]
pub enum Parsed {
    Json(Json),
    Toml(DocumentMut),
}

pub struct Config {
    pub snapshot: Snapshot,
    pub provider: Provider,
    pub original: String,
    pub text: String,
    pub parsed: Option<Parsed>,
    pub error: Option<String>,
    history: Vec<String>,
}

impl Config {
    pub fn load(path: PathBuf, provider: Provider, state_dir: PathBuf) -> Result<Self> {
        let snapshot = Snapshot::load(path, state_dir)?;
        let text = snapshot
            .text()?
            .unwrap_or(match provider {
                Provider::Claude => "{}\n",
                Provider::Codex => "",
            })
            .to_owned();
        let parsed = parse(provider, &text);
        let error = parsed.as_ref().err().map(ToString::to_string);
        Ok(Self {
            snapshot,
            provider,
            original: text.clone(),
            text,
            parsed: parsed.ok(),
            error,
            history: Vec::new(),
        })
    }
    pub fn dirty(&self) -> bool {
        self.text != self.original
    }

    pub fn matches_choice(&self, path: &[String], value: &Json) -> bool {
        match &self.parsed {
            Some(Parsed::Json(root)) => {
                path.iter().try_fold(root, |item, key| item.get(key)) == Some(value)
            }
            Some(Parsed::Toml(root)) => {
                let Some(item) = path
                    .iter()
                    .try_fold(root.as_item(), |item, key| item.get(key))
                else {
                    return false;
                };
                match value {
                    Json::String(value) => item.as_str() == Some(value),
                    Json::Bool(value) => item.as_bool() == Some(*value),
                    Json::Number(value) if value.is_f64() => item.as_float() == value.as_f64(),
                    Json::Number(value) => {
                        item.as_integer().is_some_and(|n| Some(n) == value.as_i64())
                    }
                    // 日時・テーブル・配列は、単一値の候補との照合対象にしない。
                    _ => false,
                }
            }
            None => false,
        }
    }

    pub fn replace(&mut self, text: String) -> Result<()> {
        let parsed = parse(self.provider, &text)?;
        if text == self.text {
            return Ok(());
        }
        self.history.push(self.text.clone());
        if self.history.len() > 30 {
            self.history.remove(0);
        }
        self.text = text;
        self.parsed = Some(parsed);
        self.error = None;
        Ok(())
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.history.pop() else {
            return false;
        };
        let parsed = parse(self.provider, &previous);
        self.error = parsed.as_ref().err().map(ToString::to_string);
        self.parsed = parsed.ok();
        self.text = previous;
        true
    }

    pub fn set(&mut self, path: &[String], value: &str, plain_string: bool) -> Result<()> {
        let mut parsed = self
            .parsed
            .clone()
            .ok_or_else(|| anyhow::anyhow!("全体編集で構文を修正してください"))?;
        match &mut parsed {
            Parsed::Json(root) => {
                let value = if plain_string {
                    Json::String(value.into())
                } else {
                    serde_json::from_str(value).map_err(json_error)?
                };
                set_json(root, path, Some(value))?;
            }
            Parsed::Toml(root) => {
                let value: Toml = if plain_string {
                    Toml::from(value)
                } else {
                    let fragment = format!("value = {value}");
                    let parsed = fragment
                        .parse::<DocumentMut>()
                        .map_err(|_| anyhow::anyhow!("TOMLの値として読み取れません"))?;
                    ensure!(parsed.as_table().len() == 1, "値を1つだけ入力してください");
                    parsed
                        .get("value")
                        .and_then(Item::as_value)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("TOMLの値を入力してください"))?
                };
                set_toml(root.as_table_mut(), path, Some(value))?;
            }
        }
        self.replace(render(&parsed)?)
    }

    pub fn remove(&mut self, path: &[String]) -> Result<()> {
        let mut parsed = self
            .parsed
            .clone()
            .ok_or_else(|| anyhow::anyhow!("全体編集で構文を修正してください"))?;
        match &mut parsed {
            Parsed::Json(root) => set_json(root, path, None)?,
            Parsed::Toml(root) => set_toml(root.as_table_mut(), path, None)?,
        }
        self.replace(render(&parsed)?)
    }

    pub fn save(&mut self) -> Result<Option<PathBuf>> {
        parse(self.provider, &self.text)?;
        if !self.dirty() {
            return Ok(None);
        }
        let backup = self.snapshot.save(&self.text)?;
        self.original = self.text.clone();
        Ok(backup)
    }
}

pub fn parse(provider: Provider, text: &str) -> Result<Parsed> {
    match provider {
        Provider::Claude => {
            let value: Json = serde_json::from_str(text).map_err(json_error)?;
            ensure!(
                value.is_object(),
                "JSONの最上位はオブジェクトにしてください"
            );
            Ok(Parsed::Json(value))
        }
        Provider::Codex => text
            .parse::<DocumentMut>()
            .map(Parsed::Toml)
            .map_err(|error| {
                let offset = error
                    .span()
                    .map(|span| span.start)
                    .unwrap_or(0)
                    .min(text.len());
                let prefix = &text.as_bytes()[..offset];
                let line = prefix.iter().filter(|&&c| c == b'\n').count() + 1;
                let column = prefix
                    .iter()
                    .rposition(|&c| c == b'\n')
                    .map_or(offset + 1, |pos| offset - pos);
                anyhow::anyhow!("TOMLの構文が不正です（{line}行 {column}列）")
            }),
    }
}

fn json_error(error: serde_json::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "JSONの構文が不正です（{}行 {}列）",
        error.line(),
        error.column()
    )
}

fn render(parsed: &Parsed) -> Result<String> {
    Ok(match parsed {
        Parsed::Json(value) => format!("{}\n", serde_json::to_string_pretty(value)?),
        Parsed::Toml(value) => value.to_string(),
    })
}

fn set_json(root: &mut Json, path: &[String], value: Option<Json>) -> Result<()> {
    let (key, rest) = path
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("設定キーがありません"))?;
    let object = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("親の設定値がオブジェクトではありません"))?;
    if rest.is_empty() {
        if let Some(value) = value {
            object.insert(key.clone(), value);
        } else {
            object.remove(key);
        }
    } else {
        if !object.contains_key(key) {
            if value.is_none() {
                return Ok(());
            }
            object.insert(key.clone(), Json::Object(Default::default()));
        }
        set_json(object.get_mut(key).expect("key exists"), rest, value)?;
    }
    Ok(())
}

fn set_toml(table: &mut dyn TableLike, path: &[String], value: Option<Toml>) -> Result<()> {
    let (key, rest) = path
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("設定キーがありません"))?;
    if rest.is_empty() {
        if let Some(mut value) = value {
            if let Some(previous) = table.get(key).and_then(Item::as_value) {
                *value.decor_mut() = previous.decor().clone();
            }
            if let Some(item) = table.get_mut(key) {
                *item = Item::Value(value);
            } else {
                table.insert(key, Item::Value(value));
            }
        } else {
            table.remove(key);
        }
    } else {
        if !table.contains_key(key) {
            if value.is_none() {
                return Ok(());
            }
            let mut nested = toml_edit::Table::new();
            nested.set_implicit(true);
            table.insert(key, Item::Table(nested));
        }
        let Some(nested) = table.get_mut(key).and_then(Item::as_table_like_mut) else {
            bail!("親の設定値がテーブルではありません");
        };
        set_toml(nested, rest, value)?;
    }
    Ok(())
}
