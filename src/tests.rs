use std::{fs, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::{TempDir, tempdir};

use crate::{
    app::{App, Focus, Loaded, Modal},
    document::{Config, Parsed, parse},
    editor::Editor,
    fields,
    profile::{Profile, Profiles, Provider, expand_path},
    storage::Snapshot,
    ui,
};

struct Fixture {
    directory: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            directory: tempdir().unwrap(),
        }
    }
    fn path(&self, path: &str) -> PathBuf {
        self.directory.path().join(path)
    }
    fn write(&self, path: &str, text: &str) -> PathBuf {
        let path = self.path(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
    fn config(&self, provider: Provider, text: &str) -> Config {
        let path = self.write(provider.filename(), text);
        Config::load(path, provider, self.path("state")).unwrap()
    }
    fn profiles(&self) -> Profiles {
        Profiles {
            items: vec![
                Profile {
                    name: "Claude · Default".into(),
                    provider: Provider::Claude,
                    directory: self.path(".claude"),
                },
                Profile {
                    name: "Codex · Default".into(),
                    provider: Provider::Codex,
                    directory: self.path(".codex"),
                },
            ],
            snapshot: Snapshot::load(self.path("registry/profiles.toml"), self.path("state"))
                .unwrap(),
            state_dir: self.path("state"),
        }
    }
    fn app(&self) -> App {
        App::new(self.profiles(), self.directory.path().to_owned(), None).unwrap()
    }
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
fn control(character: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
}

#[test]
fn missing_config_is_read_only_until_explicit_save() {
    let fixture = Fixture::new();
    let mut config = Config::load(
        fixture.path("new/settings.json"),
        Provider::Claude,
        fixture.path("state"),
    )
    .unwrap();
    assert!(!config.snapshot.exists());
    assert!(!config.dirty());
    config.save().unwrap();
    assert!(!fixture.path("new").exists());
    assert!(!fixture.path("state").exists());
    config.set(&path(&["model"]), "custom-model", true).unwrap();
    assert!(!fixture.path("new").exists());
    assert!(config.save().unwrap().is_none());
    assert!(fixture.path("new/settings.json").is_file());
}

#[test]
fn json_edits_preserve_unknown_keys_and_insertion_order() {
    let fixture = Fixture::new();
    let mut config = fixture.config(
        Provider::Claude,
        "{\"future\":{\"enabled\":true},\"model\":\"before\",\"last\":42}",
    );
    config.set(&path(&["model"]), "after", true).unwrap();
    let Parsed::Json(json) = config.parsed.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(json["future"]["enabled"], true);
    assert_eq!(json["last"], 42);
    assert!(config.text.find("future").unwrap() < config.text.find("model").unwrap());
    config.remove(&path(&["model"])).unwrap();
    assert!(!config.text.contains("before"));
    assert!(config.text.contains("future"));
}

#[test]
fn toml_edits_preserve_comments_and_nested_inline_tables() {
    let fixture = Fixture::new();
    let mut config = fixture.config(Provider::Codex, "# model comment\nmodel = 'before' # keep inline\nfuture = { nested = { enabled = false } }\n\n[model_providers.custom]\nbase_url = 'https://example.com/api' # keep provider\n");
    config.set(&path(&["model"]), "after", true).unwrap();
    config
        .set(&path(&["future", "nested", "enabled"]), "true", false)
        .unwrap();
    config
        .set(
            &path(&["sandbox_workspace_write", "network_access"]),
            "false",
            false,
        )
        .unwrap();
    assert!(config.text.contains("# model comment"));
    assert!(config.text.contains("# keep inline"));
    assert!(config.text.contains("# keep provider"));
    assert!(config.text.contains("enabled = true"));
    assert!(parse(Provider::Codex, &config.text).is_ok());
    config
        .remove(&path(&["future", "nested", "enabled"]))
        .unwrap();
    assert!(!config.text.contains("enabled = true"));
}

#[test]
fn literal_dots_in_keys_are_not_split_into_paths() {
    let fixture = Fixture::new();
    let mut config = fixture.config(
        Provider::Codex,
        "[projects.\"/tmp/work.space\"]\ntrust_level = \"trusted\"\n",
    );
    let field = fields::fields(&config)
        .into_iter()
        .find(|field| field.path.last().unwrap() == "trust_level")
        .unwrap();
    assert_eq!(
        field.path,
        path(&["projects", "/tmp/work.space", "trust_level"])
    );
    config.set(&field.path, "untrusted", true).unwrap();
    let Parsed::Toml(document) = config.parsed.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(
        document["projects"]["/tmp/work.space"]["trust_level"].as_str(),
        Some("untrusted")
    );
}

#[test]
fn invalid_edits_never_replace_the_working_document_or_leak_source() {
    let fixture = Fixture::new();
    let mut config = fixture.config(Provider::Codex, "model = \"before\"\n");
    let previous = config.text.clone();
    let error = config
        .replace("password = \"private-test-value\n".into())
        .unwrap_err()
        .to_string();
    assert!(!error.contains("private-test-value"));
    assert_eq!(config.text, previous);
    assert!(config.set(&path(&["model"]), "[", false).is_err());
    assert_eq!(config.text, previous);
    assert!(parse(Provider::Claude, "[]").is_err());
}

#[test]
fn broken_existing_file_can_be_repaired_and_backed_up() {
    let fixture = Fixture::new();
    let mut config = fixture.config(Provider::Claude, "{ broken");
    assert!(config.error.is_some());
    assert!(!config.dirty());
    config.replace("{\"model\":\"sonnet\"}\n".into()).unwrap();
    let backup = config.save().unwrap().unwrap();
    assert_eq!(fs::read_to_string(backup).unwrap(), "{ broken");
    assert!(config.error.is_none());
    assert!(!config.dirty());
}

#[test]
fn undo_restores_the_previous_draft_including_an_invalid_original() {
    let fixture = Fixture::new();
    let mut config = fixture.config(Provider::Claude, "{ broken");
    config.replace("{}\n".into()).unwrap();
    assert!(config.undo());
    assert_eq!(config.text, "{ broken");
    assert!(config.error.is_some());
    assert!(!config.dirty());
    assert!(!config.undo());
    config.replace("{}\n".into()).unwrap();
    config.set(&path(&["model"]), "sonnet", true).unwrap();
    assert!(config.undo());
    assert_eq!(config.text, "{}\n");
}

#[test]
fn external_changes_and_concurrent_creation_are_not_overwritten() {
    let fixture = Fixture::new();
    let file = fixture.write("settings.json", "original");
    let mut first = Snapshot::load(file.clone(), fixture.path("state")).unwrap();
    let mut second = Snapshot::load(file.clone(), fixture.path("state")).unwrap();
    first.save("first").unwrap();
    assert!(second.save("second").is_err());
    assert_eq!(fs::read_to_string(file).unwrap(), "first");
    let mut missing =
        Snapshot::load(fixture.path("new/config.toml"), fixture.path("state")).unwrap();
    fixture.write("new/config.toml", "external");
    assert!(missing.save("mine").is_err());
    assert_eq!(
        fs::read_to_string(fixture.path("new/config.toml")).unwrap(),
        "external"
    );
}

#[test]
fn backups_are_rotated_per_file_and_kept_outside_the_profile() {
    let fixture = Fixture::new();
    let file = fixture.write("profile/settings.json", "0");
    let mut snapshot = Snapshot::load(file, fixture.path("state")).unwrap();
    for number in 1..=12 {
        snapshot.save(&number.to_string()).unwrap();
    }
    assert_eq!(
        fs::read_dir(fixture.path("state/backups")).unwrap().count(),
        10
    );
    assert_eq!(fs::read_dir(fixture.path("profile")).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn symlinks_and_file_modes_survive_save_and_retargeting_is_detected() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let fixture = Fixture::new();
    let target = fixture.write("dotfiles/settings.json", "original");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    let link = fixture.path("settings.json");
    symlink(&target, &link).unwrap();
    let mut snapshot = Snapshot::load(link.clone(), fixture.path("state")).unwrap();
    let backup = snapshot.save("updated").unwrap().unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "updated");
    assert_eq!(
        fs::metadata(target).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(
        fs::metadata(backup).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let other = fixture.write("other.json", "updated");
    fs::remove_file(&link).unwrap();
    symlink(other, &link).unwrap();
    assert!(snapshot.save("overwritten").is_err());
}

#[cfg(unix)]
#[test]
fn dangling_and_credential_symlinks_are_rejected() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    symlink(fixture.path("missing"), fixture.path("dangling")).unwrap();
    assert!(Snapshot::load(fixture.path("dangling"), fixture.path("state")).is_err());
    let credentials = fixture.write("auth.json", "{}");
    symlink(credentials, fixture.path("config.toml")).unwrap();
    assert!(Snapshot::load(fixture.path("config.toml"), fixture.path("state")).is_err());
}

#[test]
fn profile_registration_is_persisted_and_removal_keeps_settings() {
    let fixture = Fixture::new();
    let mut profiles = fixture.profiles();
    let settings = fixture.write("work/settings.json", "{}");
    profiles
        .add("Work".into(), Provider::Claude, fixture.path("work"))
        .unwrap();
    assert!(
        profiles
            .add("Work".into(), Provider::Codex, fixture.path("other"))
            .is_err()
    );
    assert!(
        profiles
            .add("Duplicate".into(), Provider::Claude, fixture.path("work"))
            .is_err()
    );
    let loaded = Profiles::load(
        fixture.path("registry/profiles.toml"),
        fixture.directory.path(),
    )
    .unwrap();
    assert_eq!(loaded.items[2].name, "Work");
    assert!(profiles.remove(0).is_err());
    profiles.remove(2).unwrap();
    assert!(settings.exists());
    assert_eq!(profiles.items.len(), 2);
    assert_eq!(
        expand_path(
            std::path::Path::new("~/.claude-work/../.codex-home"),
            fixture.directory.path()
        )
        .unwrap(),
        fixture.path(".codex-home")
    );
}

#[test]
fn editor_handles_multiline_japanese_and_backspace_without_byte_splitting() {
    let mut editor = Editor::new("");
    editor.insert("日本語\nabc", true);
    assert_eq!(editor.text(), "日本語\nabc");
    editor.handle(key(KeyCode::Home), true);
    editor.handle(key(KeyCode::Backspace), true);
    assert_eq!(editor.text(), "日本語abc");
    assert_eq!(editor.visual_col(), 6);
    editor.handle(key(KeyCode::Backspace), true);
    assert_eq!(editor.text(), "日本abc");
    editor.insert("\u{1b}[31m", true);
    assert!(!editor.text().contains('\u{1b}'));
}

#[test]
fn app_preserves_drafts_across_profiles_and_requires_confirmation_to_save_or_quit() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Enter));
    let Some(Modal::Edit { editor, .. }) = &mut app.modal else {
        panic!()
    };
    editor.insert("custom-model", false);
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.dirty_count(), 1);
    assert!(!fixture.path(".claude/settings.json").exists());
    app.handle_key(key(KeyCode::Char(']')));
    app.handle_key(key(KeyCode::Char('[')));
    assert!(app.config().unwrap().text.contains("custom-model"));
    app.handle_key(key(KeyCode::Char('q')));
    assert!(!app.quit);
    assert!(app.modal.is_some());
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(control('s'));
    assert!(!fixture.path(".claude/settings.json").exists());
    app.handle_key(key(KeyCode::Enter));
    assert!(fixture.path(".claude/settings.json").is_file());
    assert_eq!(app.dirty_count(), 0);
    app.handle_key(key(KeyCode::Char('q')));
    assert!(app.quit);
}

#[test]
fn tui_add_dialog_registers_a_codex_profile() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Char('a')));
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Tab));
    for letter in "Work".chars() {
        app.handle_key(key(KeyCode::Char(letter)));
    }
    app.handle_key(key(KeyCode::Tab));
    app.handle_key(control('u'));
    // Cursor starts at the beginning, so Ctrl+K clears the prefilled ~/ path.
    app.handle_key(control('k'));
    for letter in fixture.path("work").to_string_lossy().chars() {
        app.handle_key(key(KeyCode::Char(letter)));
    }
    app.handle_key(control('s'));
    assert!(app.modal.is_none());
    assert_eq!(app.profiles.items[app.selected].name, "Work");
    assert_eq!(app.profiles.items[app.selected].provider, Provider::Codex);
    assert!(!fixture.path("work").exists());
}

#[test]
fn tui_renders_all_dialogs_and_responsive_sizes() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    fixture.write(
        ".claude/settings.json",
        "{\"env\":{\"API_KEY\":\"private-value\"}}",
    );
    // Reload after the app loaded the missing file.
    app.handle_key(key(KeyCode::Char('r')));
    for (width, height) in [(160, 45), (100, 30), (60, 16), (20, 5)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
        for code in ['?', 'a', '/', 'E'] {
            app.handle_key(key(KeyCode::Char(code)));
            terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
            app.handle_key(key(KeyCode::Esc));
        }
    }
    app.focus = Focus::Settings;
    assert!(matches!(app.documents[0], Loaded::Ready(_)));
}
