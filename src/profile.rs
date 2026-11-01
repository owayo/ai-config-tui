use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::storage::Snapshot;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
        }
    }
    pub fn filename(self) -> &'static str {
        match self {
            Self::Claude => "settings.json",
            Self::Codex => "config.toml",
        }
    }
    pub fn format(self) -> &'static str {
        match self {
            Self::Claude => "JSON",
            Self::Codex => "TOML",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub provider: Provider,
    pub directory: PathBuf,
}

impl Profile {
    pub fn path(&self) -> PathBuf {
        self.directory.join(self.provider.filename())
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    #[serde(default)]
    profiles: Vec<Profile>,
}

pub struct Profiles {
    pub items: Vec<Profile>,
    pub snapshot: Snapshot,
    pub state_dir: PathBuf,
}

impl Profiles {
    pub fn load(path: PathBuf, home: &Path) -> Result<Self> {
        let state_dir = env::var_os("XDG_STATE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".local/state"))
            .join("ai-config-tui");
        let snapshot = Snapshot::load(path, state_dir.clone())?;
        let registry: Registry = match snapshot.text()? {
            Some(text) => toml_edit::de::from_str(text)
                .map_err(|_| anyhow::anyhow!("プロファイル一覧のTOML形式が不正です"))?,
            None => Registry::default(),
        };
        let mut items = vec![
            Profile {
                name: "Claude · Default".into(),
                provider: Provider::Claude,
                directory: home.join(".claude"),
            },
            Profile {
                name: "Codex · Default".into(),
                provider: Provider::Codex,
                directory: home.join(".codex"),
            },
        ];
        for mut profile in registry.profiles {
            profile.directory = expand_path(&profile.directory, home)?;
            validate_profile(&items, &profile)?;
            items.push(profile);
        }
        // Environment overrides are visible without replacing the two default directories.
        for (key, provider) in [
            ("CLAUDE_CONFIG_DIR", Provider::Claude),
            ("CODEX_HOME", Provider::Codex),
        ] {
            if let Some(directory) = env::var_os(key).filter(|value| !value.is_empty()) {
                let directory = expand_path(Path::new(&directory), home)?;
                if !items
                    .iter()
                    .any(|item| item.provider == provider && item.directory == directory)
                {
                    items.push(Profile {
                        name: format!("{} · {key}", provider.label()),
                        provider,
                        directory,
                    });
                }
            }
        }
        Ok(Self {
            items,
            snapshot,
            state_dir,
        })
    }

    pub fn add(&mut self, name: String, provider: Provider, directory: PathBuf) -> Result<()> {
        let profile = Profile {
            name: name.trim().to_owned(),
            provider,
            directory,
        };
        validate_profile(&self.items, &profile)?;
        let mut items = self.items.clone();
        items.push(profile);
        self.persist(&items)?;
        self.items = items;
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> Result<()> {
        ensure!(index >= 2, "既定のプロファイルは解除できません");
        ensure!(index < self.items.len(), "プロファイルが見つかりません");
        let mut items = self.items.clone();
        items.remove(index);
        self.persist(&items)?;
        self.items = items;
        Ok(())
    }

    fn persist(&mut self, items: &[Profile]) -> Result<()> {
        let registry = Registry {
            profiles: items.iter().skip(2).cloned().collect(),
        };
        let text = toml_edit::ser::to_string_pretty(&registry)?;
        self.snapshot.save(&text)?;
        Ok(())
    }
}

fn validate_profile(items: &[Profile], profile: &Profile) -> Result<()> {
    ensure!(
        !profile.name.is_empty() && !profile.name.chars().any(char::is_control),
        "名前を入力してください（制御文字は使えません）"
    );
    ensure!(
        !items.iter().any(|item| item.name == profile.name),
        "同じ名前が登録されています"
    );
    ensure!(
        !items.iter().any(|item| item.provider == profile.provider
            && same_directory(&item.directory, &profile.directory)),
        "同じ種類・ディレクトリが登録されています"
    );
    ensure!(
        !profile.directory.try_exists()? || profile.directory.is_dir(),
        "プロファイルのパスはディレクトリを指定してください"
    );
    Ok(())
}

fn same_directory(left: &Path, right: &Path) -> bool {
    left == right
        || matches!((std::fs::canonicalize(left), std::fs::canonicalize(right)), (Ok(left), Ok(right)) if left == right)
}

pub fn expand_path(path: &Path, home: &Path) -> Result<PathBuf> {
    ensure!(
        !path.as_os_str().is_empty(),
        "ディレクトリを入力してください"
    );
    let path = if path == Path::new("~") {
        home.to_path_buf()
    } else if let Ok(rest) = path.strip_prefix("~") {
        home.join(rest)
    } else {
        path.to_path_buf()
    };
    let absolute = if path.is_absolute() {
        path
    } else {
        env::current_dir()?.join(path)
    };
    // Lexical normalization also makes duplicate relative spellings compare correctly.
    let mut normalized = PathBuf::new();
    for part in absolute.components() {
        use std::path::Component;
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    Ok(normalized)
}

pub fn registry_path(home: &Path) -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".config"))
        .join("ai-config-tui/profiles.toml")
}

pub fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().context("ホームディレクトリを取得できません")
}
