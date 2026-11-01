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
        .find(|field| field.path.last().unwrap() == "trust_level" && !field.template)
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
    app.handle_key(key(KeyCode::End));
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
    app.handle_key(key(KeyCode::Left));
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

fn select_field(app: &mut App, name: &str) {
    app.query.clear();
    let index = app
        .rows()
        .iter()
        .position(|field| field.key() == name && !field.template)
        .unwrap();
    app.table_state.select(Some(index));
    app.handle_key(key(KeyCode::Enter));
}

fn choose(app: &mut App, label: &str) {
    let Some(Modal::Select { options, state, .. }) = &mut app.modal else {
        panic!("expected selection dialog")
    };
    let index = options
        .iter()
        .position(|option| option.label == label)
        .unwrap();
    state.select(Some(index));
    app.handle_key(key(KeyCode::Enter));
}

#[test]
fn setting_names_and_explanations_are_searchable_and_render_without_writing_configs() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.query = "会話の自動圧縮".into();
    let field = app
        .rows()
        .into_iter()
        .find(|field| field.key() == "autoCompactEnabled")
        .unwrap();
    assert_eq!(field.label, "会話の自動圧縮");
    assert!(field.description.contains("コンテキストの上限"));
    assert!(field.value.is_none());
    assert!(!app.config().unwrap().dirty());

    app.query = "autoCompactEnabled".into();
    let mut terminal = Terminal::new(TestBackend::new(180, 42)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let mut screen = String::new();
    for row in terminal.backend().buffer().content.chunks(180) {
        let mut column = 0;
        while column < row.len() {
            let symbol = row[column].symbol();
            screen.push_str(symbol);
            column += unicode_width::UnicodeWidthStr::width(symbol).max(1);
        }
        screen.push('\n');
    }
    assert!(screen.contains("autoCompactEnabled"), "{screen}");
    assert!(screen.contains("会話の自動圧縮"), "{screen}");
    assert!(screen.contains("コンテキストの上限"), "{screen}");
    assert!(!screen.contains("公式リファレンスに記載されている設定です。"));

    app.query.clear();
    app.handle_key(key(KeyCode::Char(']')));
    app.query = "コンテキストの上限".into();
    let field = app
        .rows()
        .into_iter()
        .find(|field| field.key() == "model_context_window")
        .unwrap();
    assert_eq!(field.label, "コンテキストの上限");
    assert!(field.description.contains("トークン数"));
    assert!(!app.config().unwrap().dirty());
    assert!(!fixture.path(".claude").exists());
    assert!(!fixture.path(".codex").exists());
}

#[test]
fn dynamic_settings_keep_their_display_names_and_original_paths_when_edited() {
    let fixture = Fixture::new();
    let mut config = fixture.config(
        Provider::Codex,
        "[mcp_servers.work]\ncommand = 'tool-before'\n",
    );
    let field = fields::fields(&config)
        .into_iter()
        .find(|field| field.key() == "mcp_servers.work.command")
        .unwrap();
    assert_eq!(field.label, "MCPの起動コマンド");
    assert!(field.description.contains("標準入出力"));
    config.set(&field.path, "tool-next", true).unwrap();
    assert!(config.text.contains("[mcp_servers.work]"));
    assert!(config.text.contains("tool-next"));
    assert!(!config.text.contains("起動コマンド"));
}

#[test]
fn localized_help_matches_official_keys_and_unknown_children_do_not_borrow_parent_explanations() {
    let text: serde_json::Value =
        serde_json::from_str(include_str!("../data/settings-help.ja.json")).unwrap();
    for (provider, name) in [(Provider::Claude, "claude"), (Provider::Codex, "codex")] {
        for (key, help) in text[name].as_object().unwrap() {
            assert!(
                crate::catalog::entries(provider)
                    .iter()
                    .any(|entry| entry.key == *key),
                "setting help has no official key: {name}.{key}"
            );
            assert!(!help["label"].as_str().unwrap().is_empty());
            assert!(!help["description"].as_str().unwrap().is_empty());
        }
    }
    let (name, description) = crate::catalog::help(Provider::Codex, "tui.futureURL");
    assert_eq!(name, "端末UIの設定 / Future URL");
    assert!(description.is_empty());
    let (name, _) = crate::catalog::help(Provider::Codex, "tui.keymap.agents.archive");
    assert_eq!(name, "キー割り当て / エージェント一覧 / アーカイブ");
}

#[test]
fn official_catalog_includes_unset_settings_scopes_and_models_without_creating_files() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    assert_eq!(
        app.rows().iter().filter(|field| !field.group).count(),
        crate::catalog::entries(Provider::Claude)
            .iter()
            .filter(|entry| !entry.retired && !entry.deprecated)
            .count(),
    );
    for name in [
        "theme",
        "outputStyle",
        "autoUpdatesChannel",
        "sandbox.enabled",
    ] {
        let field = app
            .rows()
            .into_iter()
            .find(|field| field.key() == name)
            .unwrap();
        assert!(field.value.is_none());
        assert!(field.source.starts_with("https://code.claude.com/"));
        assert!(!field.choices.is_empty(), "missing choices for {name}");
    }
    let model = app
        .rows()
        .into_iter()
        .find(|field| field.key() == "model")
        .unwrap();
    assert!(model.choices.contains(&serde_json::json!("opus")));
    assert!(model.choices.contains(&serde_json::json!("sonnet")));
    assert!(model.choices.contains(&serde_json::json!("haiku")));
    let model_settings = app
        .rows()
        .into_iter()
        .find(|field| field.key() == "modelSettings")
        .unwrap();
    assert_eq!(model_settings.kind, crate::fields::Kind::Raw);
    assert!(model_settings.choices.is_empty());
    assert_eq!(
        app.rows()
            .into_iter()
            .find(|field| field.key() == "spinnerVerbs")
            .unwrap()
            .initial,
        "{}"
    );
    app.handle_key(key(KeyCode::Char(']')));
    assert!(app.rows().len() >= 302);
    assert_eq!(
        app.rows()
            .into_iter()
            .find(|field| field.key() == "tui.keymap.<context>.<action>")
            .unwrap()
            .kind,
        crate::fields::Kind::Raw
    );
    for name in [
        "tui.alternate_screen",
        "oss_provider",
        "web_search",
        "sandbox_mode",
    ] {
        let field = app
            .rows()
            .into_iter()
            .find(|field| field.key() == name)
            .unwrap();
        assert!(field.value.is_none());
        assert!(!field.source.is_empty());
        assert!(!field.choices.is_empty());
    }
    assert!(!fixture.path(".claude").exists());
    assert!(!fixture.path(".codex").exists());
    assert_eq!(app.dirty_count(), 0);
}

#[test]
fn claude_settings_are_grouped_parent_first_and_codex_stays_flat() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    let rows = app.rows();
    let permissions = rows
        .iter()
        .position(|field| field.key() == "permissions")
        .unwrap();
    assert_eq!(rows[permissions + 1].key(), "permissions.defaultMode");
    assert_eq!(rows[permissions + 2].key(), "permissions.allow");
    assert!(rows[permissions].tree_prefix.is_empty());
    assert!(rows[permissions + 1].tree_prefix.starts_with("├─ "));
    for (index, field) in rows
        .iter()
        .enumerate()
        .filter(|(_, field)| field.path.len() > 1)
    {
        let parent = &field.path[..field.path.len() - 1];
        assert!(rows[..index].iter().any(|row| row.path == parent));
        assert!(!field.tree_prefix.is_empty());
    }
    let sandbox = rows
        .iter()
        .position(|field| field.key() == "sandbox")
        .unwrap();
    let end = rows[sandbox + 1..]
        .iter()
        .position(|field| field.path.len() == 1)
        .map_or(rows.len(), |offset| sandbox + 1 + offset);
    assert!(
        rows[sandbox + 1..end]
            .iter()
            .all(|field| field.path[0] == "sandbox")
    );
    let network = rows
        .iter()
        .position(|field| field.key() == "sandbox.network")
        .unwrap();
    assert!(rows[network + 1].path.starts_with(&rows[network].path));
    assert!(
        rows[network + 1].tree_prefix.chars().count() > rows[network].tree_prefix.chars().count()
    );
    assert_eq!(
        rows.iter().filter(|field| !field.group).count(),
        crate::catalog::entries(Provider::Claude)
            .iter()
            .filter(|entry| !entry.retired && !entry.deprecated)
            .count()
    );

    app.handle_key(key(KeyCode::Char(']')));
    assert!(
        app.rows()
            .iter()
            .all(|field| !field.group && field.tree_prefix.is_empty())
    );
    assert!(!fixture.path(".claude").exists());
    assert!(!fixture.path(".codex").exists());
}

#[test]
fn hierarchy_search_keeps_ancestors_selects_the_match_and_renders_the_full_key() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.focus = Focus::Settings;
    app.modal = Some(Modal::Search(Editor::new("接続許可ドメイン")));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        app.current_field().unwrap().key(),
        "sandbox.network.allowedDomains"
    );
    let keys = app
        .rows()
        .iter()
        .map(|field| field.key())
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            "sandbox",
            "sandbox.network",
            "sandbox.network.allowedDomains"
        ]
    );
    assert_eq!(app.table_state.selected(), Some(2));
    for (width, height) in [(60, 16), (180, 42)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
        if width == 180 {
            let mut screen = String::new();
            for row in terminal.backend().buffer().content.chunks(width as usize) {
                let mut column = 0;
                while column < row.len() {
                    let symbol = row[column].symbol();
                    screen.push_str(symbol);
                    column += unicode_width::UnicodeWidthStr::width(symbol).max(1);
                }
                screen.push('\n');
            }
            assert!(screen.contains("└─ allowedDomains"), "{screen}");
            assert!(
                screen.contains("キー: sandbox.network.allowedDomains"),
                "{screen}"
            );
        }
    }
    app.handle_key(key(KeyCode::Enter));
    let Some(Modal::Edit {
        path: Some(path), ..
    }) = &app.modal
    else {
        panic!()
    };
    assert_eq!(path, &["sandbox", "network", "allowedDomains"]);
    assert!(!app.config().unwrap().dirty());
}

#[test]
fn hierarchy_search_matches_parent_names_and_handles_no_results() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.focus = Focus::Settings;
    let label = app
        .rows()
        .into_iter()
        .find(|field| field.key() == "sandbox.network")
        .unwrap()
        .label;
    app.modal = Some(Modal::Search(Editor::new(&label)));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.current_field().unwrap().key(), "sandbox.network");
    assert!(
        app.rows()
            .iter()
            .any(|field| field.key() == "sandbox.network.allowUnixSockets")
    );
    assert!(
        app.rows()
            .iter()
            .any(|field| field.key() == "sandbox.network.allowedDomains")
    );
    app.modal = Some(Modal::Search(Editor::new("no-such-setting-example-001")));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.rows().is_empty());
    assert!(app.current_field().is_none());
    app.handle_key(key(KeyCode::Enter));
    assert!(app.modal.is_none());
    assert!(!app.config().unwrap().dirty());
}

#[test]
fn virtual_parent_groups_cannot_be_edited_toggled_or_reset() {
    let fixture = Fixture::new();
    fixture.write(
        ".claude/settings.json",
        r#"{"remote":{"defaultEnvironmentId":"example"}}"#,
    );
    let mut app = fixture.app();
    app.focus = Focus::Settings;
    let original = app.config().unwrap().text.clone();
    let index = app
        .rows()
        .iter()
        .position(|field| field.path == path(&["remote"]))
        .unwrap();
    app.table_state.select(Some(index));
    let field = app.current_field().unwrap();
    assert!(field.group);
    assert!(field.value.is_none());
    assert!(field.disabled_reason().is_some());
    for code in [KeyCode::Enter, KeyCode::Char(' '), KeyCode::Char('d')] {
        app.handle_key(key(code));
        assert!(app.modal.is_none());
        assert_eq!(app.config().unwrap().text, original);
        assert!(!app.config().unwrap().dirty());
    }
}

#[test]
fn hierarchy_preserves_literal_dots_and_edits_the_selected_path() {
    let fixture = Fixture::new();
    fixture.write(
        ".claude/settings.json",
        r#"{"permissions.allow":"literal","permissions":{"allow":["Read(*)"]}}"#,
    );
    let mut app = fixture.app();
    app.focus = Focus::Settings;
    let rows = app.rows();
    let literal = rows
        .iter()
        .position(|field| field.path == path(&["permissions.allow"]))
        .unwrap();
    let nested = rows
        .iter()
        .position(|field| field.path == path(&["permissions", "allow"]))
        .unwrap();
    assert!(rows[literal].tree_prefix.is_empty());
    assert!(!rows[nested].tree_prefix.is_empty());
    assert!(!rows[literal].group);
    app.table_state.select(Some(literal));
    app.handle_key(key(KeyCode::Enter));
    let Some(Modal::Edit {
        editor,
        path: Some(selected),
        ..
    }) = &mut app.modal
    else {
        panic!()
    };
    assert_eq!(selected, &path(&["permissions.allow"]));
    *editor = Editor::new("updated");
    app.handle_key(control('s'));
    let result: serde_json::Value = serde_json::from_str(&app.config().unwrap().text).unwrap();
    assert_eq!(result["permissions.allow"], "updated");
    assert_eq!(
        result["permissions"]["allow"],
        serde_json::json!(["Read(*)"])
    );
}

#[test]
fn retired_settings_are_hidden_unless_the_key_is_present() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    assert!(app.rows().iter().all(|field| !field.retired));
    assert!(
        !app.rows()
            .iter()
            .any(|field| field.key() == "keybindingFlavor")
    );
    assert!(
        !app.rows()
            .iter()
            .any(|field| field.key() == "includeCoAuthoredBy")
    );
    assert!(
        !app.rows()
            .iter()
            .any(|field| field.key() == "disableArtifact")
    );
    app.handle_key(key(KeyCode::Char(']')));
    assert!(app.rows().iter().all(|field| !field.retired));
    assert!(!app.rows().iter().any(|field| field.key() == "personality"));
    for value in [
        serde_json::json!(false),
        serde_json::json!(null),
        serde_json::json!(""),
        serde_json::json!([]),
        serde_json::json!({}),
        serde_json::json!(0),
    ] {
        let original = serde_json::json!({"keybindingFlavor": value}).to_string();
        let config = fixture.config(Provider::Claude, &original);
        let field = fields::fields(&config)
            .into_iter()
            .find(|field| field.key() == "keybindingFlavor")
            .unwrap();
        assert!(field.retired);
        assert!(field.value.is_some());
        assert_eq!(config.text, original);
        assert!(!config.dirty());
    }
}

#[test]
fn configured_retired_settings_precede_active_settings_and_keep_full_paths() {
    let fixture = Fixture::new();
    fixture.write(".claude/settings.json", r#"{"model":"opus","keybindingFlavor":"readline","permissionExplainerEnabled":false,"teammateDefaultModel":null,"taskOutputMaxChars":0}"#);
    fixture.write(".codex/config.toml", "model = 'sample-model'\npersonality = ''\n[features]\npersonality = false\n[features.guardianv2]\nasync_classifier_mode = 'conversations'\nthread_context = false\n[features.multi_agent_v2]\nusage_hint_enabled = false\n");
    let mut app = fixture.app();
    for expected in [4, 5] {
        let rows = app.rows();
        assert!(
            rows[..expected]
                .iter()
                .all(|field| field.retired && field.value.is_some())
        );
        assert!(rows[expected..].iter().all(|field| !field.retired));
        assert_eq!(rows[expected].key(), "model");
        assert!(app.current_field().unwrap().retired);
        assert!(!app.config().unwrap().dirty());
        if expected == 4 {
            app.handle_key(key(KeyCode::Char(']')));
        }
    }
    app.modal = Some(Modal::Search(Editor::new("async_classifier_mode")));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.rows().len(), 1);
    let field = app.current_field().unwrap();
    assert_eq!(
        field.path,
        path(&["features", "guardianv2", "async_classifier_mode"])
    );
    assert!(field.retired);
    assert!(field.tree_prefix.is_empty());
}

#[test]
fn retired_rows_and_detail_keys_are_red_even_when_selected() {
    let fixture = Fixture::new();
    fixture.write(
        ".claude/settings.json",
        r#"{"keybindingFlavor":"readline"}"#,
    );
    let mut app = fixture.app();
    app.focus = Focus::Settings;
    let mut terminal = Terminal::new(TestBackend::new(180, 42)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let expected = ratatui::style::Color::Rgb(248, 134, 141);
    let text = "keybindingFlavor";
    let mut matches = 0;
    for row in terminal.backend().buffer().content.chunks(180) {
        for cells in row.windows(text.len()) {
            if cells.iter().map(|cell| cell.symbol()).collect::<String>() == text {
                assert!(cells.iter().all(|cell| cell.fg == expected));
                if matches == 0 {
                    let value = row
                        .windows("readline".len())
                        .find(|cells| {
                            cells.iter().map(|cell| cell.symbol()).collect::<String>() == "readline"
                        })
                        .unwrap();
                    assert!(value.iter().all(|cell| cell.fg == expected));
                }
                matches += 1;
            }
        }
    }
    assert!(
        matches >= 2,
        "the setting key must be red in both the table and detail"
    );
}

#[test]
fn deprecated_settings_are_hidden_unless_present_and_preserve_falsy_values() {
    let fixture = Fixture::new();
    let app = fixture.app();
    assert!(
        app.rows()
            .iter()
            .all(|field| field.warning_label().is_none())
    );
    for value in [
        serde_json::json!(false),
        serde_json::json!(null),
        serde_json::json!(""),
        serde_json::json!([]),
    ] {
        let original = serde_json::json!({"includeCoAuthoredBy":value}).to_string();
        let config = fixture.config(Provider::Claude, &original);
        let field = fields::fields(&config)
            .into_iter()
            .find(|field| field.key() == "includeCoAuthoredBy")
            .unwrap();
        assert!(field.deprecated && !field.retired);
        assert_eq!(field.warning_label(), Some("非推奨"));
        assert!(field.value.is_some());
        assert_eq!(config.text, original);
        assert!(!config.dirty());
    }
}

#[test]
fn deprecated_settings_follow_retired_settings_and_render_in_orange() {
    let fixture = Fixture::new();
    fixture.write(".claude/settings.json", r#"{"model":"opus","keybindingFlavor":"readline","voiceEnabled":false,"includeCoAuthoredBy":false,"disableArtifact":true}"#);
    fixture.write(
        ".codex/config.toml",
        "web_search = 'live'\n[features]\nweb_search = false\n",
    );
    let mut app = fixture.app();
    let rows = app.rows();
    assert!(rows[0].retired);
    assert!(
        rows[1..4]
            .iter()
            .all(|field| field.deprecated && !field.retired)
    );
    assert!(
        rows[4..]
            .iter()
            .all(|field| field.warning_label().is_none())
    );
    assert_eq!(rows[4].key(), "model");
    assert!(rows[..4].iter().all(|field| field.tree_prefix.is_empty()));
    assert!(!app.config().unwrap().dirty());

    // Keep the key out of the search title when checking rendered field colors.
    app.query = rows
        .iter()
        .find(|field| field.key() == "includeCoAuthoredBy")
        .unwrap()
        .label
        .clone();
    app.table_state.select(Some(0));
    app.focus = Focus::Settings;
    let mut terminal = Terminal::new(TestBackend::new(180, 42)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let orange = ratatui::style::Color::Rgb(255, 165, 0);
    let text = "includeCoAuthoredBy";
    let mut matches = 0;
    for row in terminal.backend().buffer().content.chunks(180) {
        for cells in row.windows(text.len()) {
            if cells.iter().map(|cell| cell.symbol()).collect::<String>() == text {
                assert!(cells.iter().all(|cell| cell.fg == orange));
                if matches == 0 {
                    let value = row
                        .windows("false".len())
                        .find(|cells| {
                            cells.iter().map(|cell| cell.symbol()).collect::<String>() == "false"
                        })
                        .unwrap();
                    assert!(value.iter().all(|cell| cell.fg == orange));
                }
                matches += 1;
            }
        }
    }
    assert!(matches >= 2, "table and detail must both use orange");

    app.handle_key(key(KeyCode::Char(']')));
    assert_eq!(app.current_field().unwrap().key(), "features.web_search");
    assert!(app.current_field().unwrap().deprecated);
    assert_eq!(app.current_field().unwrap().value.as_deref(), Some("false"));
    assert!(
        app.rows()
            .iter()
            .any(|field| field.key() == "web_search" && !field.deprecated)
    );
    assert!(!app.config().unwrap().dirty());
}

#[test]
fn deprecated_settings_remain_editable_and_reset_and_undo_update_visibility() {
    let fixture = Fixture::new();
    let original = r#"{"includeCoAuthoredBy":false}"#;
    let file = fixture.write(".claude/settings.json", original);
    let mut app = fixture.app();
    select_field(&mut app, "includeCoAuthoredBy");
    choose(&mut app, "true");
    assert!(app.current_field().unwrap().deprecated);
    assert_eq!(app.current_field().unwrap().value.as_deref(), Some("true"));
    app.handle_key(key(KeyCode::Char('u')));
    assert_eq!(app.current_field().unwrap().value.as_deref(), Some("false"));
    app.focus = Focus::Settings;
    app.query = "includeCoAuthoredBy".into();
    app.handle_key(key(KeyCode::Char('d')));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.rows().is_empty());
    app.handle_key(key(KeyCode::Char('u')));
    assert_eq!(app.rows().len(), 1);
    assert!(app.current_field().unwrap().deprecated);
    assert!(!app.config().unwrap().dirty());
    assert_eq!(fs::read_to_string(file).unwrap(), original);
}

#[test]
fn resetting_a_retired_setting_hides_it_and_undo_restores_it() {
    let fixture = Fixture::new();
    let original = r#"{"keybindingFlavor":"readline","model":"opus"}"#;
    let file = fixture.write(".claude/settings.json", original);
    let mut app = fixture.app();
    app.focus = Focus::Settings;
    app.query = "keybindingFlavor".into();
    app.handle_key(key(KeyCode::Char('d')));
    assert!(matches!(app.modal, Some(Modal::Confirm { .. })));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.rows().is_empty());
    assert!(app.config().unwrap().dirty());
    assert_eq!(fs::read_to_string(&file).unwrap(), original);
    app.handle_key(key(KeyCode::Char('u')));
    assert_eq!(app.rows().len(), 1);
    assert!(app.current_field().unwrap().retired);
    assert!(!app.config().unwrap().dirty());
    assert_eq!(fs::read_to_string(&file).unwrap(), original);
}

#[test]
fn model_picker_stages_an_alias_and_cancel_preserves_the_existing_value() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    select_field(&mut app, "model");
    choose(&mut app, "opus");
    assert!(app.config().unwrap().text.contains("\"model\": \"opus\""));
    assert!(!fixture.path(".claude/settings.json").exists());
    select_field(&mut app, "model");
    let Some(Modal::Select { options, state, .. }) = &app.modal else {
        panic!()
    };
    assert_eq!(options[state.selected().unwrap()].label, "opus");
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Esc));
    assert!(app.config().unwrap().text.contains("\"model\": \"opus\""));
    select_field(&mut app, "model");
    choose(&mut app, "未設定（このキーを削除）");
    assert!(!app.config().unwrap().text.contains("model"));
}

#[test]
fn selecting_booleans_writes_boolean_values_and_unset_does_not_write_a_default() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    select_field(&mut app, "alwaysThinkingEnabled");
    choose(&mut app, "false");
    let Parsed::Json(document) = app.config().unwrap().parsed.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(document["alwaysThinkingEnabled"], false);
    select_field(&mut app, "alwaysThinkingEnabled");
    choose(&mut app, "未設定（このキーを削除）");
    assert_eq!(app.dirty_count(), 0);
    assert!(!fixture.path(".claude/settings.json").exists());
}

#[test]
fn unknown_current_toml_union_value_is_preserved_byte_for_byte() {
    let fixture = Fixture::new();
    let original = "[tui]\nnotifications = ['agent-turn-complete'] # preserve quote and comment\n";
    fixture.write(".codex/config.toml", original);
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Char(']')));
    select_field(&mut app, "tui.notifications");
    let Some(Modal::Select { options, state, .. }) = &app.modal else {
        panic!()
    };
    assert!(
        options[state.selected().unwrap()]
            .label
            .contains("現在の値")
    );
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.config().unwrap().text, original);
    assert_eq!(app.dirty_count(), 0);
    select_field(&mut app, "tui.notifications");
    choose(&mut app, "false");
    let Parsed::Toml(document) = app.config().unwrap().parsed.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(document["tui"]["notifications"].as_bool(), Some(false));
}

#[test]
fn existing_named_entries_inherit_their_template_choices_and_sources() {
    let fixture = Fixture::new();
    let config = fixture.config(Provider::Codex, "[projects.\"/tmp/work.space\"]\ntrust_level = 'trusted'\n[model_providers.custom]\nbase_url = 'https://example.com/api'\n");
    let rows = fields::fields(&config);
    let concrete = rows
        .iter()
        .find(|field| field.path == path(&["projects", "/tmp/work.space", "trust_level"]))
        .unwrap();
    assert!(!concrete.template);
    assert!(concrete.disabled_reason().is_none());
    assert!(!concrete.source.is_empty());
    assert!(concrete.choices.contains(&serde_json::json!("trusted")));
    assert!(concrete.choices.contains(&serde_json::json!("untrusted")));
    let provider = rows
        .iter()
        .find(|field| field.key() == "model_provider")
        .unwrap();
    assert!(provider.choices.contains(&serde_json::json!("custom")));
}

#[test]
fn managed_only_settings_cannot_be_written_by_the_picker_or_space_shortcut() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    select_field(&mut app, "allowManagedHooksOnly");
    assert!(app.modal.is_none());
    assert!(app.is_error);
    app.handle_key(key(KeyCode::Char(' ')));
    assert_eq!(app.dirty_count(), 0);
    assert!(app.status.contains("管理者"));
}

#[test]
fn model_picker_can_enter_a_provider_specific_id() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    select_field(&mut app, "model");
    choose(&mut app, "任意の値を入力…");
    let Some(Modal::Edit { editor, .. }) = &mut app.modal else {
        panic!()
    };
    editor.insert("custom-provider-model", false);
    app.handle_key(key(KeyCode::Enter));
    select_field(&mut app, "model");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.config().unwrap().text.contains("custom-provider-model"));
}

#[test]
fn selection_dialog_renders_and_scrolls_at_the_minimum_terminal_size() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    select_field(&mut app, "model");
    let mut terminal = Terminal::new(TestBackend::new(60, 16)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    app.handle_key(key(KeyCode::End));
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut content = String::new();
    for row in buffer.content.chunks(60) {
        let mut column = 0;
        while column < row.len() {
            let symbol = row[column].symbol();
            content.push_str(symbol);
            column += unicode_width::UnicodeWidthStr::width(symbol).max(1);
        }
        content.push('\n');
    }
    assert!(content.contains("任意の値を入力"), "{content}");
    app.handle_key(key(KeyCode::Enter));
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
}

#[test]
fn provider_detection_uses_config_files_then_directory_names() {
    let fixture = Fixture::new();
    fixture.write("work/config.toml", "model = 'custom'\n");
    assert_eq!(
        Provider::detect(&fixture.path("work")).unwrap(),
        Some(Provider::Codex)
    );
    fixture.write("home/settings.json", "{}");
    assert_eq!(
        Provider::detect(&fixture.path("home")).unwrap(),
        Some(Provider::Claude)
    );
    assert_eq!(
        Provider::detect(&fixture.path(".codex-work")).unwrap(),
        Some(Provider::Codex)
    );
    assert_eq!(
        Provider::detect(&fixture.path(".claude-home")).unwrap(),
        Some(Provider::Claude)
    );
    assert_eq!(Provider::detect(&fixture.path("unknown")).unwrap(), None);
    assert_eq!(Provider::detect(&fixture.path(".codexish")).unwrap(), None);
    fixture.write(".claude-other/config.toml", "");
    assert_eq!(
        Provider::detect(&fixture.path(".claude-other")).unwrap(),
        Some(Provider::Codex)
    );
    fixture.write(".codex-mixed/settings.json", "{}");
    fixture.write(".codex-mixed/config.toml", "");
    assert_eq!(
        Provider::detect(&fixture.path(".codex-mixed")).unwrap(),
        None
    );
    assert!(Provider::resolve(None, &fixture.path(".codex-mixed")).is_err());
    assert_eq!(
        Provider::resolve(Some(Provider::Claude), &fixture.path("work")).unwrap(),
        Provider::Claude
    );
}

#[test]
fn add_dialog_auto_detects_codex_without_changing_the_type_control() {
    let fixture = Fixture::new();
    fixture.write("work/config.toml", "model = 'custom'\n");
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Char('a')));
    let Some(Modal::Add {
        provider,
        name,
        directory,
        ..
    }) = &mut app.modal
    else {
        panic!()
    };
    assert!(provider.is_none());
    *name = Editor::new("Work");
    *directory = Editor::new(fixture.path("work").to_str().unwrap());
    app.handle_key(control('s'));
    assert!(app.modal.is_none());
    let profile = &app.profiles.items[app.selected];
    assert_eq!(profile.provider, Provider::Codex);
    assert_eq!(profile.path(), fixture.path("work/config.toml"));
    assert_eq!(app.config().unwrap().provider, Provider::Codex);
    assert!(app.rows().iter().any(|field| field.key() == "sandbox_mode"));
    assert!(!fixture.path("work/settings.json").exists());
    let loaded = Profiles::load(
        fixture.path("registry/profiles.toml"),
        fixture.directory.path(),
    )
    .unwrap();
    assert_eq!(loaded.items[2].provider, Provider::Codex);
}

#[test]
fn add_dialog_refuses_unknown_auto_types_and_respects_manual_selection() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Char('a')));
    let Some(Modal::Add {
        name, directory, ..
    }) = &mut app.modal
    else {
        panic!()
    };
    *name = Editor::new("Work");
    *directory = Editor::new(fixture.path("work").to_str().unwrap());
    app.handle_key(control('s'));
    assert_eq!(app.profiles.items.len(), 2);
    assert!(!fixture.path("registry/profiles.toml").exists());
    let Some(Modal::Add { provider, .. }) = &mut app.modal else {
        panic!()
    };
    *provider = Some(Provider::Codex);
    app.handle_key(control('s'));
    assert!(app.modal.is_none());
    assert_eq!(app.profiles.items[app.selected].provider, Provider::Codex);
}

#[test]
fn changing_a_stored_provider_preserves_settings_and_creates_a_registry_backup() {
    let fixture = Fixture::new();
    fixture.write("work/config.toml", "model = 'custom'\n");
    let mut profiles = fixture.profiles();
    profiles
        .add("Work".into(), Provider::Claude, fixture.path("work"))
        .unwrap();
    let before = fs::read(fixture.path("registry/profiles.toml")).unwrap();
    profiles.set_provider(2, Provider::Codex).unwrap();
    assert_eq!(profiles.items[2].provider, Provider::Codex);
    assert_eq!(profiles.items[2].path(), fixture.path("work/config.toml"));
    assert_eq!(
        fs::read_to_string(fixture.path("work/config.toml")).unwrap(),
        "model = 'custom'\n"
    );
    assert!(!fixture.path("work/settings.json").exists());
    let backups = fs::read_dir(fixture.path("state/backups"))
        .unwrap()
        .map(|entry| fs::read(entry.unwrap().path()).unwrap())
        .collect::<Vec<_>>();
    assert!(backups.contains(&before));
    assert!(profiles.set_provider(0, Provider::Codex).is_err());
}

#[test]
fn profile_display_order_groups_providers_and_keeps_registration_order() {
    let fixture = Fixture::new();
    let mut profiles = fixture.profiles();
    for (name, provider, directory) in [
        ("CodexWork", Provider::Codex, "codex-work"),
        ("ClaudeWork", Provider::Claude, "claude-work"),
        ("ClaudeHome", Provider::Claude, "claude-home"),
    ] {
        profiles
            .add(name.into(), provider, fixture.path(directory))
            .unwrap();
    }
    let registered = fs::read(fixture.path("registry/profiles.toml")).unwrap();
    assert_eq!(profiles.display_order(), vec![0, 3, 4, 1, 2]);
    assert_eq!(profiles.items[0].name, "Claude · Default");
    assert_eq!(profiles.items[1].name, "Codex · Default");
    assert_eq!(profiles.items[2].name, "CodexWork");
    assert_eq!(
        fs::read(fixture.path("registry/profiles.toml")).unwrap(),
        registered
    );

    let mut app = App::new(
        profiles,
        fixture.directory.path().to_owned(),
        Some("Codex · Default"),
    )
    .unwrap();
    assert_eq!(app.selected, 1);
    assert_eq!(app.profile_state.selected(), Some(3));
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    let positions = [
        "Claude · Default",
        "ClaudeWork",
        "ClaudeHome",
        "Codex · Default",
        "CodexWork",
    ]
    .map(|name| text.find(name).unwrap());
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn profile_navigation_follows_the_grouped_order_and_preserves_drafts() {
    let fixture = Fixture::new();
    fixture.write("claude-work/settings.json", "{\"model\":\"sonnet\"}");
    let mut profiles = fixture.profiles();
    profiles
        .add(
            "ClaudeWork".into(),
            Provider::Claude,
            fixture.path("claude-work"),
        )
        .unwrap();
    profiles
        .add(
            "CodexWork".into(),
            Provider::Codex,
            fixture.path("codex-work"),
        )
        .unwrap();
    let mut app = App::new(profiles, fixture.directory.path().to_owned(), None).unwrap();
    app.handle_key(key(KeyCode::Char(']')));
    assert_eq!(app.selected, 2);
    assert_eq!(app.profile_state.selected(), Some(1));
    assert_eq!(app.profiles.items[app.selected].name, "ClaudeWork");
    app.config_mut()
        .unwrap()
        .set(&path(&["model"]), "opus", true)
        .unwrap();
    app.handle_key(key(KeyCode::Char(']')));
    assert_eq!(app.selected, 1);
    assert_eq!(app.profile_state.selected(), Some(2));
    assert_eq!(app.config().unwrap().provider, Provider::Codex);
    app.handle_key(key(KeyCode::Char('[')));
    assert!(app.config().unwrap().text.contains("opus"));
    assert!(app.config().unwrap().dirty());
    assert_eq!(
        fs::read_to_string(fixture.path("claude-work/settings.json")).unwrap(),
        "{\"model\":\"sonnet\"}"
    );
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.selected, 1);
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.selected, 2);
    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.selected, 3);
    assert_eq!(app.profile_state.selected(), Some(3));
    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(app.selected, 0);
    assert_eq!(app.dirty_count(), 1);
}

#[test]
fn adding_a_claude_profile_selects_its_grouped_position_without_moving_drafts() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Char(']')));
    app.config_mut()
        .unwrap()
        .set(&path(&["model"]), "custom-codex-model", true)
        .unwrap();
    app.handle_key(key(KeyCode::Char('a')));
    let Some(Modal::Add {
        provider,
        name,
        directory,
        ..
    }) = &mut app.modal
    else {
        panic!()
    };
    *provider = Some(Provider::Claude);
    *name = Editor::new("ClaudeWork");
    *directory = Editor::new(fixture.path("claude-work").to_str().unwrap());
    app.handle_key(control('s'));
    assert_eq!(app.selected, 2);
    assert_eq!(app.profile_state.selected(), Some(1));
    assert_eq!(app.config().unwrap().provider, Provider::Claude);
    app.handle_key(key(KeyCode::Char(']')));
    assert_eq!(app.selected, 1);
    assert_eq!(app.config().unwrap().provider, Provider::Codex);
    assert!(app.config().unwrap().text.contains("custom-codex-model"));
    assert_eq!(app.dirty_count(), 1);
}

#[test]
fn removing_a_grouped_profile_keeps_defaults_protected_and_documents_aligned() {
    let fixture = Fixture::new();
    let mut profiles = fixture.profiles();
    profiles
        .add(
            "ClaudeWork".into(),
            Provider::Claude,
            fixture.path("claude-work"),
        )
        .unwrap();
    profiles
        .add(
            "CodexWork".into(),
            Provider::Codex,
            fixture.path("codex-work"),
        )
        .unwrap();
    let mut app = App::new(
        profiles,
        fixture.directory.path().to_owned(),
        Some("CodexWork"),
    )
    .unwrap();
    app.config_mut()
        .unwrap()
        .set(&path(&["model"]), "custom-model", true)
        .unwrap();
    app.handle_key(key(KeyCode::Char('[')));
    app.handle_key(key(KeyCode::Char('[')));
    assert_eq!(app.profiles.items[app.selected].name, "ClaudeWork");
    assert_eq!(app.profile_state.selected(), Some(1));
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Char('x')));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.profiles.items[app.selected].name, "Codex · Default");
    assert_eq!(app.profile_state.selected(), Some(1));
    app.handle_key(key(KeyCode::Char('x')));
    assert!(app.modal.is_none());
    assert!(app.is_error);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.profiles.items[app.selected].name, "CodexWork");
    assert_eq!(app.config().unwrap().provider, Provider::Codex);
    assert!(app.config().unwrap().text.contains("custom-model"));
    assert!(
        app.config()
            .unwrap()
            .snapshot
            .path
            .ends_with("codex-work/config.toml")
    );
}
