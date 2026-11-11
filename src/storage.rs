use std::{
    fs::{self, File, OpenOptions},
    hash::{Hash, Hasher},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use tempfile::Builder;

const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// 指定されたパスと、シンボリックリンクを解決した実際の保存先を保持する。
#[derive(Debug)]
pub struct Snapshot {
    pub path: PathBuf,
    target: PathBuf,
    original: Option<Vec<u8>>,
    state_dir: PathBuf,
}

impl Snapshot {
    pub fn load(path: PathBuf, state_dir: PathBuf) -> Result<Self> {
        let target = resolve_target(&path)?;
        ensure!(
            !matches!(
                target.file_name().and_then(|name| name.to_str()),
                Some("auth.json" | ".credentials.json")
            ),
            "認証ファイルは編集対象にできません"
        );
        let original = read_optional(&target)?;
        Ok(Self {
            path,
            target,
            original,
            state_dir,
        })
    }

    pub fn text(&self) -> Result<Option<&str>> {
        self.original
            .as_deref()
            .map(std::str::from_utf8)
            .transpose()
            .context("設定ファイルがUTF-8ではありません")
    }

    pub fn exists(&self) -> bool {
        self.original.is_some()
    }

    pub fn save(&mut self, text: &str) -> Result<Option<PathBuf>> {
        ensure!(
            text.len() as u64 <= MAX_BYTES,
            "設定ファイルは5 MiB以内にしてください"
        );
        let parent = self
            .target
            .parent()
            .context("保存先ディレクトリがありません")?;
        private_dir(parent).context("保存先ディレクトリを作成できません")?;
        let filename = self
            .target
            .file_name()
            .context("保存先ファイル名がありません")?
            .to_string_lossy();
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.target.hash(&mut hash);
        let identity = format!("{:016x}", hash.finish());
        let lock_dir = self.state_dir.join("locks");
        private_dir(&lock_dir)?;
        let lock_path = lock_dir.join(format!("{identity}.lock"));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options
            .open(lock_path)
            .context("保存用ロックを開けません")?;
        lock.try_lock()
            .context("別のai-config-tuiが保存中です。少し待って再実行してください")?;
        self.check_unchanged()?;

        let permissions = if self.original.is_some() {
            let metadata = fs::metadata(&self.target)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                // geteuid は事前条件がなく、失敗しない。
                ensure!(
                    metadata.uid() == unsafe { libc::geteuid() },
                    "所有者が異なる設定ファイルには保存できません"
                );
            }
            ensure!(
                !metadata.permissions().readonly(),
                "設定ファイルが読み取り専用です"
            );
            Some(metadata.permissions())
        } else {
            None
        };
        let mut temporary = Builder::new()
            .prefix(".ai-config-tui-")
            .tempfile_in(parent)?;
        temporary.write_all(text.as_bytes())?;
        if let Some(permissions) = permissions {
            temporary.as_file().set_permissions(permissions)?;
        }
        temporary.as_file().sync_all()?;

        let backup = if let Some(original) = &self.original {
            let backup_dir = self.state_dir.join("backups");
            private_dir(&backup_dir)?;
            let mut file = Builder::new()
                .prefix(&format!("{identity}-{filename}-"))
                .suffix(".bak")
                .tempfile_in(&backup_dir)?;
            file.write_all(original)?;
            file.as_file().sync_all()?;
            let (_, path) = file.keep().context("バックアップを保存できません")?;
            Some(path)
        } else {
            None
        };

        // 一時ファイルとバックアップを書いた後、置き換えの直前に再確認する。
        self.check_unchanged()?;
        if self.original.is_some() {
            temporary
                .persist(&self.target)
                .context("設定ファイルを置き換えられません")?;
        } else {
            temporary
                .persist_noclobber(&self.target)
                .context("設定ファイルを新規作成できません")?;
        }
        self.original = Some(text.as_bytes().to_vec());
        prune_backups(
            &self.state_dir.join("backups"),
            &format!("{identity}-{filename}-"),
        );
        Ok(backup)
    }

    fn check_unchanged(&self) -> Result<()> {
        ensure!(
            resolve_target(&self.path)? == self.target,
            "リンク先が変更されました。再読み込みしてから編集してください"
        );
        ensure!(
            read_optional(&self.target)? == self.original,
            "他のプロセスが設定ファイルを変更しました。再読み込みしてから編集してください"
        );
        Ok(())
    }
}

fn private_dir(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

fn prune_backups(directory: &Path, prefix: &str) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut files = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
        .filter_map(|entry| {
            entry
                .metadata()
                .ok()
                .filter(|metadata| metadata.is_file())
                .and_then(|metadata| metadata.modified().ok())
                .map(|modified| (modified, entry.path()))
        })
        .collect::<Vec<_>>();
    files.sort();
    let count = files.len().saturating_sub(10);
    for (_, path) in files.into_iter().take(count) {
        let _ = fs::remove_file(path);
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::metadata(path) {
        Ok(metadata) => ensure!(metadata.is_file(), "設定のパスが通常ファイルではありません"),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("設定ファイルの情報を取得できません"),
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("設定ファイルを開けません"),
    };
    ensure!(
        file.metadata()?.is_file(),
        "設定のパスが通常ファイルではありません"
    );
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "設定ファイルが5 MiBを超えています"
    );
    Ok(Some(bytes))
}

fn resolve_target(path: &Path) -> Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(target) => Ok(target),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            match fs::symlink_metadata(path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!("リンク先が存在しません")
                }
                Ok(_) => bail!("設定のパスを解決できません"),
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            let parent = path.parent().context("パスを解決できません")?;
            let name = path.file_name().context("ファイル名がありません")?;
            Ok(resolve_target(parent)?.join(name))
        }
        Err(error) => Err(error).context("設定のパスを解決できません"),
    }
}
