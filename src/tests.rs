use std::{fs, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::{TempDir, tempdir};

use crate::{
    app::{App, Focus, Loaded, Modal},
    document::{Config, Parsed, parse},
    editor::Editor,
    fields,
    profile::{Profile, ProfileOrigin, Profiles, Provider, expand_path},
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
                    origin: ProfileOrigin::Builtin,
                },
                Profile {
                    name: "Codex · Default".into(),
                    provider: Provider::Codex,
                    directory: self.path(".codex"),
                    origin: ProfileOrigin::Builtin,
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

fn screen_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for row in buffer.content.chunks(buffer.area.width as usize) {
        let mut column = 0;
        while column < row.len() {
            let symbol = row[column].symbol();
            text.push_str(symbol);
            column += unicode_width::UnicodeWidthStr::width(symbol).max(1);
        }
        text.push('\n');
    }
    text
}

#[test]
fn web_search_picker_explains_modes_scrolls_and_saves_only_the_chosen_value() {
    let fixture = Fixture::new();
    let original = "web_search = 'cached' # keep this comment\n";
    let file = fixture.write(".codex/config.toml", original);
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Char(']')));
    select_field(&mut app, "web_search");
    let mut terminal = Terminal::new(TestBackend::new(60, 16)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let cached = screen_text(&terminal);
    assert!(cached.contains("OpenAIが管理"), "{cached}");
    app.handle_key(key(KeyCode::End));
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let live = screen_text(&terminal);
    assert!(live.contains("live"), "{live}");
    assert!(live.contains("Webから最新"), "{live}");
    assert_eq!(app.config().unwrap().text, original);
    assert_eq!(app.dirty_count(), 0);
    for _ in 0..30 {
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL));
        terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    }
    let source = screen_text(&terminal);
    assert!(
        source.contains("learn.chatgpt.com/docs/web-search"),
        "{source}"
    );
    let Some(Modal::Select { options, state, .. }) = &app.modal else {
        panic!()
    };
    assert_eq!(options[state.selected().unwrap()].label, "live");
    app.handle_key(key(KeyCode::Up));
    let Some(Modal::Select { scroll, .. }) = &app.modal else {
        panic!()
    };
    assert_eq!(*scroll, 0);
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    assert!(screen_text(&terminal).contains("検索索引を通じて"));
    choose(&mut app, "live");
    assert_eq!(fs::read_to_string(&file).unwrap(), original);
    assert!(app.config().unwrap().dirty());
    app.handle_key(control('s'));
    app.handle_key(key(KeyCode::Enter));
    let saved = fs::read_to_string(file).unwrap();
    let Parsed::Toml(document) = parse(Provider::Codex, &saved).unwrap() else {
        panic!()
    };
    assert_eq!(document["web_search"].as_str(), Some("live"));
    assert!(saved.contains("# keep this comment"));
}

#[test]
fn choice_help_uses_provider_path_and_json_type_and_keeps_literal_dots() {
    use crate::catalog::choice_help;
    use serde_json::json;

    let history = choice_help(
        Provider::Codex,
        &path(&["history", "persistence"]),
        &json!("none"),
    )
    .unwrap();
    let opener = choice_help(Provider::Codex, &path(&["file_opener"]), &json!("none")).unwrap();
    assert!(history.description.contains("history.jsonl"));
    assert!(opener.description.contains("URI"));
    assert_ne!(history.description, opener.description);
    let browser = path(&["browser_use", "allow_history_access"]);
    assert!(choice_help(Provider::Codex, &browser, &json!(false)).is_some());
    assert!(choice_help(Provider::Codex, &browser, &json!("false")).is_none());
    assert!(choice_help(Provider::Claude, &browser, &json!(false)).is_none());
    assert!(
        choice_help(
            Provider::Codex,
            &path(&["web_search"]),
            &json!("future-mode")
        )
        .is_none()
    );
    let project = path(&["projects", "/tmp/work.space", "trust_level"]);
    assert!(choice_help(Provider::Codex, &project, &json!("trusted")).is_some());
    let wrong_path = path(&["projects", "/tmp/work", "space", "trust_level"]);
    assert!(choice_help(Provider::Codex, &wrong_path, &json!("trusted")).is_none());
}

#[test]
fn picker_operations_and_unexplained_values_do_not_claim_official_help() {
    let fixture = Fixture::new();
    let original = "[tui]\nnotifications = ['agent-turn-complete'] # keep\n";
    fixture.write(".codex/config.toml", original);
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Char(']')));
    select_field(&mut app, "tui.notifications");
    let Some(Modal::Select { options, state, .. }) = &app.modal else {
        panic!()
    };
    let current = &options[state.selected().unwrap()];
    assert!(current.help.description.contains("そのまま保持"));
    assert!(current.help.source.is_empty());
    let unset = &options[0];
    assert!(unset.help.description.contains("キーを削除"));
    let unexplained = options
        .iter()
        .find(|option| option.label == "false")
        .unwrap();
    assert!(unexplained.help.description.contains("まだ登録"));
    assert!(unexplained.help.source.is_empty());
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.config().unwrap().text, original);
    assert_eq!(app.dirty_count(), 0);
    app.handle_key(key(KeyCode::Char('[')));
    select_field(&mut app, "model");
    let Some(Modal::Select { options, .. }) = &app.modal else {
        panic!()
    };
    let custom = options.last().unwrap();
    assert!(custom.help.description.contains("編集画面で入力"));
    assert!(custom.help.source.is_empty());
    assert!(
        options
            .iter()
            .find(|option| option.label == "opus")
            .unwrap()
            .help
            .source
            .starts_with("https://code.claude.com/")
    );
}

#[test]
fn curated_choice_explanations_match_catalog_values_and_have_official_sources() {
    use std::collections::HashSet;

    let help: serde_json::Value =
        serde_json::from_str(include_str!("../data/choice-help.ja.json")).unwrap();
    assert!(!help["verified_at"].as_str().unwrap().is_empty());
    // 公式の資料サイトと、公式リポジトリの更新履歴・リリースノートだけを出典にする。
    for (name, provider, source_prefixes) in [
        (
            "claude",
            Provider::Claude,
            [
                "https://code.claude.com/",
                "https://github.com/anthropics/claude-code/",
            ],
        ),
        (
            "codex",
            Provider::Codex,
            [
                "https://learn.chatgpt.com/",
                "https://github.com/openai/codex/",
            ],
        ),
    ] {
        let mut seen = HashSet::new();
        for group in help[name].as_array().unwrap() {
            let source = group["source"].as_str().unwrap();
            assert!(
                source_prefixes
                    .iter()
                    .any(|prefix| source.starts_with(prefix)),
                "{name}: {source}"
            );
            for key in group["keys"].as_array().unwrap() {
                let key = key.as_str().unwrap();
                let entry = crate::catalog::entries(provider)
                    .iter()
                    .find(|entry| entry.key == key)
                    .unwrap_or_else(|| panic!("{name}: {key}"));
                for choice in group["choices"].as_array().unwrap() {
                    assert!(
                        entry.choices.contains(&choice["value"]),
                        "{name}: {key}: {choice}"
                    );
                    assert!(!choice["description"].as_str().unwrap().trim().is_empty());
                    assert!(
                        seen.insert((key, choice["value"].to_string())),
                        "duplicate: {key}"
                    );
                }
            }
        }
    }
}

#[test]
fn incorrectly_typed_current_values_keep_their_own_option_and_are_not_explained_as_booleans() {
    let fixture = Fixture::new();
    let original = "[browser_use]\nallow_history_access = 'false' # preserve\n";
    fixture.write(".codex/config.toml", original);
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Char(']')));
    select_field(&mut app, "browser_use.allow_history_access");
    let Some(Modal::Select { options, state, .. }) = &app.modal else {
        panic!()
    };
    let current = &options[state.selected().unwrap()];
    assert!(current.label.contains("候補外"));
    assert!(current.help.source.is_empty());
    assert!(
        options
            .iter()
            .find(|option| option.label == "false")
            .unwrap()
            .help
            .description
            .contains("履歴へのアクセスを制限")
    );
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.config().unwrap().text, original);
    assert_eq!(app.dirty_count(), 0);
    select_field(&mut app, "browser_use.allow_history_access");
    choose(&mut app, "false");
    let Parsed::Toml(document) = app.config().unwrap().parsed.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(
        document["browser_use"]["allow_history_access"].as_bool(),
        Some(false)
    );
    assert!(app.config().unwrap().dirty());
}

#[test]
fn selecting_the_same_typed_value_keeps_json_and_toml_formatting_and_drafts() {
    let fixture = Fixture::new();
    let json = "{ \"model\": \"opus\", \"custom\": 7 }\n";
    let toml = "web_search = 'cached' # keep formatting\n";
    fixture.write(".claude/settings.json", json);
    fixture.write(".codex/config.toml", toml);
    let mut app = fixture.app();
    select_field(&mut app, "model");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.config().unwrap().text, json);
    assert!(!app.config().unwrap().dirty());
    app.handle_key(key(KeyCode::Char(']')));
    select_field(&mut app, "web_search");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.config().unwrap().text, toml);
    assert!(!app.config().unwrap().dirty());
    app.config_mut()
        .unwrap()
        .set(&path(&["custom"]), "true", false)
        .unwrap();
    let draft = app.config().unwrap().text.clone();
    select_field(&mut app, "web_search");
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL));
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.config().unwrap().text, draft);
    assert!(app.config().unwrap().dirty());
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
fn removing_json_keys_preserves_the_order_of_remaining_keys() {
    let fixture = Fixture::new();
    let original = r#"{"first":1,"second":2,"third":3,"nested":{"first":1,"second":2,"third":3}}"#;
    for parts in [&["first"][..], &["nested", "first"][..]] {
        let mut config = fixture.config(Provider::Claude, original);
        config.remove(&path(parts)).unwrap();
        let Parsed::Json(json) = config.parsed.as_ref().unwrap() else {
            panic!()
        };
        let object = if parts.len() == 1 {
            json
        } else {
            &json["nested"]
        };
        let expected = if parts.len() == 1 {
            vec!["second", "third", "nested"]
        } else {
            vec!["second", "third"]
        };
        assert_eq!(
            object
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(config.undo());
        assert_eq!(config.text, original);
    }
}

#[test]
fn unchanged_json_edits_preserve_formatting_and_undo_history() {
    let fixture = Fixture::new();
    let original = r#"{"language":"Japanese","nested":{"enabled":true}}"#;
    let mut config = fixture.config(Provider::Claude, original);
    config.remove(&path(&["model"])).unwrap();
    config.set(&path(&["language"]), "Japanese", true).unwrap();
    config
        .set(&path(&["nested"]), r#"{"enabled":true}"#, false)
        .unwrap();
    assert_eq!(config.text, original);
    assert!(!config.dirty());
    assert!(!config.undo());
    config.set(&path(&["language"]), "English", true).unwrap();
    let changed = config.text.clone();
    config.remove(&path(&["missing", "child"])).unwrap();
    assert_eq!(config.text, changed);
    assert!(config.undo());
    assert_eq!(config.text, original);
}

#[test]
fn unset_numeric_settings_open_empty_and_require_an_explicit_value() {
    for (provider, name, initial) in [
        (Provider::Claude, "cleanupPeriodDays", "Claude · Default"),
        (Provider::Codex, "model_context_window", "Codex · Default"),
    ] {
        let fixture = Fixture::new();
        let mut app = App::new(
            fixture.profiles(),
            fixture.directory.path().to_owned(),
            Some(initial),
        )
        .unwrap();
        select_field(&mut app, name);
        let Some(Modal::Edit { editor, .. }) = &app.modal else {
            panic!()
        };
        assert_eq!(editor.text(), "");
        app.handle_key(control('s'));
        assert!(matches!(
            &app.modal,
            Some(Modal::Edit { error: Some(_), .. })
        ));
        assert_eq!(app.dirty_count(), 0);
        let Some(Modal::Edit { editor, .. }) = &mut app.modal else {
            panic!()
        };
        editor.insert("30", false);
        app.handle_key(control('s'));
        assert!(app.modal.is_none());
        assert_eq!(app.current_field().unwrap().value.as_deref(), Some("30"));
        assert_eq!(app.config().unwrap().provider, provider);
    }
}

#[test]
fn invalid_profiles_do_not_block_other_profiles_and_can_be_reloaded_after_repair() {
    let fixture = Fixture::new();
    let mut profiles = fixture.profiles();
    profiles
        .add("Work".into(), Provider::Claude, fixture.path("work"))
        .unwrap();
    fixture.write("work", "keep");
    let mut app = App::new(profiles, fixture.directory.path().to_owned(), Some("Work")).unwrap();
    assert!(matches!(&app.documents[app.selected], Loaded::Failed(_)));
    app.handle_key(key(KeyCode::Char('[')));
    assert!(app.config().is_some());
    app.handle_key(key(KeyCode::Char(']')));
    fs::remove_file(fixture.path("work")).unwrap();
    fixture.write("work/settings.json", "{}");
    app.handle_key(key(KeyCode::Char('r')));
    assert!(app.config().is_some());
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
fn removing_toml_keys_keeps_standalone_and_inline_comments() {
    for (original, parts, comment) in [
        (
            "# 共通設定\nmodel = 'before'\napproval_policy = 'never'\n",
            vec!["model"],
            "# 共通設定",
        ),
        (
            "model = 'before'\n\n# 権限の注記\napproval_policy = 'never'\nsandbox_mode = 'read-only'\n",
            vec!["approval_policy"],
            "# 権限の注記",
        ),
        (
            "# 最後の注記\nmodel = 'before' # 行末の注記\n",
            vec!["model"],
            "# 最後の注記",
        ),
        (
            "# テーブル直前の注記\nmodel = 'before'\n[tui]\nnotifications = true\n",
            vec!["model"],
            "# テーブル直前の注記",
        ),
        (
            "[tui]\n# 子項目の注記\nnotifications = true\n# 末尾の注記\n",
            vec!["tui", "notifications"],
            "# 子項目の注記",
        ),
        (
            "# ドット付きキーの注記\ntui.notifications = true\nmodel = 'before'\n",
            vec!["tui", "notifications"],
            "# ドット付きキーの注記",
        ),
    ] {
        let fixture = Fixture::new();
        let mut config = fixture.config(Provider::Codex, original);
        config.remove(&path(&parts)).unwrap();
        assert!(config.text.contains(comment), "{}", config.text);
        if original.contains("# 行末の注記") {
            assert!(config.text.contains("# 行末の注記"));
        }
        if original.contains("# 末尾の注記") {
            assert!(config.text.contains("# 末尾の注記"));
        }
        assert!(parse(Provider::Codex, &config.text).is_ok());
        assert!(config.undo());
        assert_eq!(config.text, original);
    }
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
fn snapshot_rejects_invalid_utf8_and_oversized_files_without_writing() {
    let fixture = Fixture::new();
    let invalid = fixture.path("invalid.json");
    fs::write(&invalid, [0xff]).unwrap();
    let snapshot = Snapshot::load(invalid.clone(), fixture.path("state")).unwrap();
    assert!(snapshot.text().is_err());
    assert!(Config::load(invalid, Provider::Claude, fixture.path("state")).is_err());

    let limit = 5 * 1024 * 1024;
    let file = fixture.path("boundary.txt");
    let boundary = "x".repeat(limit);
    fs::write(&file, &boundary).unwrap();
    let mut snapshot = Snapshot::load(file.clone(), fixture.path("state")).unwrap();
    assert_eq!(snapshot.text().unwrap(), Some(boundary.as_str()));
    assert!(snapshot.save(&(boundary.clone() + "x")).is_err());
    assert_eq!(fs::read_to_string(&file).unwrap(), boundary);
    assert!(!fixture.path("state").exists());
    snapshot.save(&boundary).unwrap();
    fs::write(&file, boundary + "x").unwrap();
    assert!(Snapshot::load(file, fixture.path("state")).is_err());
}

#[test]
fn contended_save_lock_preserves_the_file_and_releases_after_failure() {
    let fixture = Fixture::new();
    let file = fixture.write("settings.json", "original");
    let mut snapshot = Snapshot::load(file.clone(), fixture.path("state")).unwrap();
    snapshot.save("first").unwrap();
    let lock_path = fs::read_dir(fixture.path("state/locks"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)
        .unwrap();
    lock.try_lock().unwrap();
    assert!(snapshot.save("second").is_err());
    assert_eq!(fs::read_to_string(&file).unwrap(), "first");
    drop(lock);
    snapshot.save("second").unwrap();
    assert_eq!(fs::read_to_string(file).unwrap(), "second");
}

#[cfg(unix)]
#[test]
fn read_only_settings_are_not_replaced() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let file = fixture.write("settings.json", "original");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o400)).unwrap();
    let mut snapshot = Snapshot::load(file.clone(), fixture.path("state")).unwrap();
    assert!(snapshot.save("changed").is_err());
    assert_eq!(fs::read_to_string(&file).unwrap(), "original");
    assert_eq!(
        fs::metadata(file).unwrap().permissions().mode() & 0o777,
        0o400
    );
    assert!(!fixture.path("state/backups").exists());
}

#[test]
fn setting_labels_replace_terminal_control_characters() {
    let fixture = Fixture::new();
    fixture.write(".claude/settings.json", r#"{"custom\u001b[31m":"value"}"#);
    let mut app = fixture.app();
    // 正規表記のキーでは、制御文字をJSONのエスケープで表す。
    select_field(&mut app, r#""custom\u001b[31m""#);
    let mut terminal = Terminal::new(TestBackend::new(180, 45)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    assert!(
        !screen_text(&terminal)
            .chars()
            .any(|character| character.is_control() && character != '\n')
    );
}

#[test]
fn nested_environment_values_are_masked_until_revealed() {
    let fixture = Fixture::new();
    fixture.write(
        ".codex/config.toml",
        "[mcp_servers.work]\ncommand = 'example-command'\n[mcp_servers.work.env]\nCUSTOM_VALUE = 'private-environment-value'\n",
    );
    let mut app = App::new(
        fixture.profiles(),
        fixture.directory.path().to_owned(),
        Some("Codex · Default"),
    )
    .unwrap();
    select_field(&mut app, "mcp_servers.work.env.CUSTOM_VALUE");
    app.handle_key(key(KeyCode::Esc));
    let mut terminal = Terminal::new(TestBackend::new(180, 45)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    assert!(!screen_text(&terminal).contains("private-environment-value"));
    app.query = "mcp_servers.work.command".into();
    let index = app
        .rows()
        .iter()
        .position(|field| field.key() == "mcp_servers.work.command")
        .unwrap();
    app.table_state.select(Some(index));
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    assert!(screen_text(&terminal).contains("example-command"));
    assert!(!screen_text(&terminal).contains("private-environment-value"));
    app.query.clear();
    select_field(&mut app, "mcp_servers.work.env.CUSTOM_VALUE");
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Char('h')));
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    assert!(screen_text(&terminal).contains("private-environment-value"));
}

#[test]
fn ancestor_values_stay_masked_when_sensitive_children_are_filtered() {
    let fixture = Fixture::new();
    fixture.write(
        ".codex/config.toml",
        "[tui]\nnotifications = true\napi_key = 'hidden123'\n",
    );
    let mut app = App::new(
        fixture.profiles(),
        fixture.directory.path().to_owned(),
        Some("Codex · Default"),
    )
    .unwrap();
    app.query = "tui.notifications".into();
    let rows = app.rows();
    assert!(rows.iter().all(|field| field.key() != "tui.api_key"));
    let index = rows.iter().position(|field| field.key() == "tui").unwrap();
    assert!(rows[index].value.as_deref().unwrap().contains("hidden123"));
    app.table_state.select(Some(index));
    let mut terminal = Terminal::new(TestBackend::new(180, 45)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    assert!(!screen_text(&terminal).contains("hidden123"));
    app.handle_key(key(KeyCode::Char('h')));
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    assert!(screen_text(&terminal).contains("hidden123"));
}

#[test]
fn shell_environment_overrides_and_api_key_headers_are_masked() {
    for (text, name) in [
        (
            "[shell_environment_policy.set]\nCUSTOM_VALUE = 'hidden123'\n",
            "shell_environment_policy.set.CUSTOM_VALUE",
        ),
        (
            "[mcp_servers.work.http_headers]\nX-Api-Key = 'hidden123'\n",
            "mcp_servers.work.http_headers.X-Api-Key",
        ),
        (
            "[model_providers.work.http_headers]\nCookie = 'hidden123'\n",
            "model_providers.work.http_headers.Cookie",
        ),
        (
            "[otel.exporter.otlp-http.headers]\nx-vendor-credential = 'hidden123'\n",
            "otel.exporter.otlp-http.headers.x-vendor-credential",
        ),
    ] {
        let fixture = Fixture::new();
        fixture.write(".codex/config.toml", text);
        let mut app = App::new(
            fixture.profiles(),
            fixture.directory.path().to_owned(),
            Some("Codex · Default"),
        )
        .unwrap();
        select_field(&mut app, name);
        app.handle_key(key(KeyCode::Esc));
        let mut terminal = Terminal::new(TestBackend::new(180, 45)).unwrap();
        terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
        assert!(!screen_text(&terminal).contains("hidden123"));
        app.handle_key(key(KeyCode::Char('h')));
        terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
        assert!(screen_text(&terminal).contains("hidden123"));
    }
}

#[test]
fn json_item_edits_keep_unknown_numbers_without_rounding() {
    let fixture = Fixture::new();
    let original = r#"{"model":"before","large":12345678901234567890123,"precise":0.12345678901234567890123,"nested":{"tiny":1.234567890123456789e-100}}"#;
    let mut config = fixture.config(Provider::Claude, original);
    config.set(&path(&["model"]), "after", true).unwrap();
    for literal in [
        "12345678901234567890123",
        "0.12345678901234567890123",
        "1.234567890123456789e-100",
    ] {
        assert!(config.text.contains(literal), "number changed: {literal}");
    }
    config.save().unwrap();
    assert_eq!(
        fs::read_to_string(fixture.path("settings.json")).unwrap(),
        config.text
    );
    assert!(config.undo());
    assert_eq!(config.text, original);
}

#[test]
fn numeric_toml_values_exclude_outer_comments_and_keep_them_when_saved() {
    let fixture = Fixture::new();
    let mut config = fixture.config(
        Provider::Codex,
        "tool_output_token_limit = 1000 # existing limit\nextra = 0x10 # extra limit\n",
    );
    let rows = fields::fields(&config);
    for (key, expected) in [("tool_output_token_limit", "1000"), ("extra", "0x10")] {
        let field = rows.iter().find(|field| field.key() == key).unwrap();
        assert_eq!(field.value.as_deref(), Some(expected));
    }
    config
        .set(&path(&["tool_output_token_limit"]), "2000", false)
        .unwrap();
    assert!(config.text.contains("2000 # existing limit"));
    assert!(config.text.contains("0x10 # extra limit"));
}

#[test]
fn numeric_limits_are_visible_while_unknown_numeric_secrets_stay_masked() {
    let fixture = Fixture::new();
    let config = fixture.config(Provider::Codex,
        "tool_output_token_limit = 1000\nunknown_token = 123456\n[mcp_servers.work.env]\nTOKEN = '654321'\n");
    let rows = fields::fields(&config);
    for (key, sensitive) in [
        ("tool_output_token_limit", false),
        ("unknown_token", true),
        ("mcp_servers.work.env.TOKEN", true),
    ] {
        assert_eq!(
            rows.iter()
                .find(|field| field.key() == key)
                .unwrap()
                .sensitive,
            sensitive,
            "{key}"
        );
    }
}

#[test]
fn array_templates_do_not_describe_named_toml_tables() {
    let fixture = Fixture::new();
    let config = fixture.config(
        Provider::Codex,
        "[skills.config.work]\npath = '/tmp/example-skill'\n",
    );
    let field = fields::fields(&config)
        .into_iter()
        .find(|field| field.key() == "skills.config.work.path")
        .unwrap();
    assert!(!field.common);
    assert!(field.source.is_empty());
}

#[test]
fn mcp_readiness_picker_keeps_quoted_names_comments_and_unrelated_settings() {
    let fixture = Fixture::new();
    let original = "[mcp_servers.\"work.example\"]\nstartup_readiness = 'connection' # 接続の完了を待つ\nunknown_value = 42\n";
    let file = fixture.write(".codex/config.toml", original);
    let mut app = App::new(
        fixture.profiles(),
        fixture.directory.path().to_owned(),
        Some("Codex · Default"),
    )
    .unwrap();
    select_field(&mut app, "mcp_servers.\"work.example\".startup_readiness");
    let Some(Modal::Select { options, .. }) = &app.modal else {
        panic!("候補の選択画面が開いていません");
    };
    let choice = options
        .iter()
        .find(|option| option.label == "catalog")
        .unwrap();
    assert!(choice.help.description.contains("キャッシュ"));
    choose(&mut app, "catalog");
    app.config_mut().unwrap().save().unwrap();
    assert_eq!(
        fs::read_to_string(file).unwrap(),
        original.replace("'connection'", "\"catalog\"")
    );
}

#[test]
fn canonical_keys_keep_dotted_quoted_and_template_parts_whole() {
    use crate::catalog::{key_path, key_text, parse_key};
    for (key, parts) in [
        (
            "notice.\"hide_gpt-5.1-codex-max_migration_prompt\"",
            path(&["notice", "hide_gpt-5.1-codex-max_migration_prompt"]),
        ),
        (
            "permissions.<name>.filesystem.\":workspace_roots\".<subpath-or-glob>",
            path(&[
                "permissions",
                "<name>",
                "filesystem",
                ":workspace_roots",
                "<subpath-or-glob>",
            ]),
        ),
        (
            "hooks.<Event>[].hooks[].async",
            path(&["hooks", "<Event>[]", "hooks[]", "async"]),
        ),
        (
            "mcp_servers.\"work.example\".command",
            path(&["mcp_servers", "work.example", "command"]),
        ),
    ] {
        assert_eq!(key_path(key), parts, "{key}");
        assert_eq!(key_text(&parts), key);
    }
    // 引用符・バックスラッシュ・制御文字・日本語・空の名前も1つの部品のまま往復する。
    let unusual = path(&["env", "a\"b\\c", "改行\nを含む", "", "*", "x y"]);
    let text = key_text(&unusual);
    assert_eq!(parse_key(&text), Some(unusual));
    // 正規表記でない入力は分割を諦めて、ドットで区切るだけにする。
    for broken in ["a..b", "a.", "\"unterminated", "a.\"b\"c", "a b.c"] {
        assert!(parse_key(broken).is_none(), "{broken}");
        assert_eq!(key_path(broken), broken.split('.').collect::<Vec<_>>());
    }
}

#[test]
fn bundled_keys_are_canonical_so_names_with_dots_stay_one_part() {
    use crate::catalog::{entries, key_text, parse_key};
    let canonical = |key: &str| parse_key(key).is_some_and(|parts| key_text(&parts) == key);
    for provider in [Provider::Claude, Provider::Codex] {
        for entry in entries(provider) {
            assert!(canonical(&entry.key), "catalog key: {}", entry.key);
        }
    }
    let help: serde_json::Value =
        serde_json::from_str(include_str!("../data/settings-help.ja.json")).unwrap();
    let choices: serde_json::Value =
        serde_json::from_str(include_str!("../data/choice-help.ja.json")).unwrap();
    for name in ["claude", "codex"] {
        for key in help[name].as_object().unwrap().keys() {
            assert!(canonical(key), "help key: {name}.{key}");
        }
        for group in choices[name].as_array().unwrap() {
            for key in group["keys"].as_array().unwrap() {
                let key = key.as_str().unwrap();
                assert!(canonical(key), "choice help key: {name}.{key}");
            }
        }
    }
    let notice = entries(Provider::Codex)
        .iter()
        .find(|entry| entry.key.starts_with("notice.\"hide_gpt-5.1"))
        .expect("the migration notice keeps its dotted name");
    assert_eq!(
        crate::catalog::key_path(&notice.key),
        path(&["notice", "hide_gpt-5.1-codex-max_migration_prompt"])
    );
}

#[test]
fn dotted_codex_notice_is_matched_toggled_and_saved_as_one_quoted_key() {
    let fixture = Fixture::new();
    let original =
        "# 移行の案内\n[notice]\n\"hide_gpt-5.1-codex-max_migration_prompt\" = true # 既読\n";
    let file = fixture.write(".codex/config.toml", original);
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Char(']')));
    app.focus = crate::app::Focus::Settings;
    let key_name = "notice.\"hide_gpt-5.1-codex-max_migration_prompt\"";
    let rows = app.rows();
    let matching = rows
        .iter()
        .filter(|field| field.path == path(&["notice", "hide_gpt-5.1-codex-max_migration_prompt"]))
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "公式の項目と追加設定に分かれています");
    assert!(matching[0].common);
    assert_eq!(matching[0].key(), key_name);
    assert_eq!(matching[0].value.as_deref(), Some("true"));
    assert!(
        !rows
            .iter()
            .any(|field| field.path.iter().any(|part| part == "hide_gpt-5"))
    );
    // 引用符なしでドットをつないだ表記でも検索できる。
    app.query = "notice.hide_gpt-5.1-codex-max".into();
    assert!(app.rows().iter().any(|field| field.key() == key_name));
    app.query.clear();
    let index = app
        .rows()
        .iter()
        .position(|field| field.key() == key_name)
        .unwrap();
    app.table_state.select(Some(index));
    app.handle_key(key(KeyCode::Char(' ')));
    app.config_mut().unwrap().save().unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        original.replace("= true", "= false")
    );

    // 未設定から切り替えると、1つの引用されたキーとして書き込む。
    let empty = Fixture::new();
    let file = empty.write(".codex/config.toml", "");
    let mut app = empty.app();
    app.handle_key(key(KeyCode::Char(']')));
    app.focus = crate::app::Focus::Settings;
    let index = app
        .rows()
        .iter()
        .position(|field| field.key() == key_name)
        .unwrap();
    app.table_state.select(Some(index));
    app.handle_key(key(KeyCode::Char(' ')));
    app.config_mut().unwrap().save().unwrap();
    let saved = fs::read_to_string(&file).unwrap();
    let document = saved.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(
        document["notice"]["hide_gpt-5.1-codex-max_migration_prompt"].as_bool(),
        Some(true),
        "{saved}"
    );
    assert!(document["notice"].get("hide_gpt-5").is_none(), "{saved}");
}

#[test]
fn claude_string_or_array_setting_uses_json_input_and_preserves_unknown_keys() {
    let fixture = Fixture::new();
    let file = fixture.write(
        ".claude/settings.json",
        r#"{"forceLoginOrgUUID":"before","unknown":"keep"}"#,
    );
    let mut app = fixture.app();
    select_field(&mut app, "forceLoginOrgUUID");
    let Some(Modal::Edit { editor, plain, .. }) = &mut app.modal else {
        panic!("JSONの入力画面が開いていません");
    };
    assert!(!*plain);
    *editor = Editor::new(
        r#"["00000000-0000-0000-0000-000000000001","00000000-0000-0000-0000-000000000002"]"#,
    );
    app.handle_key(control('s'));
    assert!(app.modal.is_none());
    app.config_mut().unwrap().save().unwrap();
    let saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
    assert_eq!(saved["forceLoginOrgUUID"].as_array().unwrap().len(), 2);
    assert_eq!(saved["unknown"], "keep");
}

#[test]
#[ignore = "設定一覧の性能を手動で計測する"]
fn benchmark_settings_rows() {
    use std::{hint::black_box, time::Instant};

    for provider in [Provider::Claude, Provider::Codex] {
        for extra_count in [0, 1000] {
            let fixture = Fixture::new();
            let text = match provider {
                Provider::Claude => serde_json::to_string(
                    &(0..extra_count)
                        .map(|index| (format!("extra_{index}"), serde_json::json!(index)))
                        .collect::<serde_json::Map<_, _>>(),
                )
                .unwrap(),
                Provider::Codex => (0..extra_count)
                    .map(|index| format!("extra_{index} = {index}\n"))
                    .collect::<String>(),
            };
            let profile = match provider {
                Provider::Claude => (".claude/settings.json", "Claude · Default"),
                Provider::Codex => (".codex/config.toml", "Codex · Default"),
            };
            fixture.write(profile.0, &text);
            let app = App::new(
                fixture.profiles(),
                fixture.directory.path().to_owned(),
                Some(profile.1),
            )
            .unwrap();
            for _ in 0..5 {
                black_box(app.rows());
            }
            let mut samples = Vec::new();
            for _ in 0..25 {
                let start = Instant::now();
                black_box(app.rows());
                samples.push(start.elapsed().as_micros());
            }
            eprintln!("{provider:?} 追加{extra_count}項目: {samples:?} µs");
            samples.sort_unstable();
            eprintln!(
                "中央値 {} µs、最小 {} µs、最大 {} µs",
                samples[12], samples[0], samples[24]
            );
        }
    }
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
fn editor_cursor_counts_the_visible_replacement_for_control_characters() {
    let original = "model = 'custom'\r\n";
    let mut editor = Editor::new(original);
    editor.handle(key(KeyCode::End), true);
    assert_eq!(
        editor.visual_col(),
        unicode_width::UnicodeWidthStr::width("model = 'custom'�")
    );
    assert_eq!(editor.text(), original);

    let mut editor = Editor::new("日\t\u{1b}本");
    assert_eq!(
        editor.visual_col(),
        unicode_width::UnicodeWidthStr::width("日    �本")
    );
    editor.handle(key(KeyCode::Backspace), true);
    assert_eq!(
        editor.visual_col(),
        unicode_width::UnicodeWidthStr::width("日    �")
    );
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
    // カーソルが先頭にあるため、Ctrl+Kで初期値の ~/ を消す。
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
    // アプリが未作成ファイルを読み込んだ後なので、作成した内容を再読込する。
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
fn settings_are_grouped_parent_first_for_both_providers() {
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
    let rows = app.rows();
    for (index, field) in rows
        .iter()
        .enumerate()
        .filter(|(_, field)| field.path.len() > 1)
    {
        let parent = &field.path[..field.path.len() - 1];
        assert!(rows[..index].iter().any(|row| row.path == parent));
        assert!(!field.tree_prefix.is_empty());
    }
    assert!(!fixture.path(".claude").exists());
    assert!(!fixture.path(".codex").exists());
}

#[test]
fn codex_browser_settings_render_as_a_tree_and_edit_the_original_path() {
    for original in [
        None,
        Some(
            "[browser_use]\nallow_history_access = false # keep\nunknown = 7\ncustom_toggle = true # extra\n",
        ),
    ] {
        let fixture = Fixture::new();
        if let Some(original) = original {
            fixture.write(".codex/config.toml", original);
        }
        let mut app = fixture.app();
        app.handle_key(key(KeyCode::Char(']')));
        app.focus = Focus::Settings;
        app.modal = Some(Modal::Search(Editor::new(
            "browser_use.allow_history_access",
        )));
        app.handle_key(key(KeyCode::Enter));
        let rows = app.rows();
        assert_eq!(
            rows.iter().map(|field| field.key()).collect::<Vec<_>>(),
            ["browser_use", "browser_use.allow_history_access"]
        );
        assert!(rows[0].tree_prefix.is_empty());
        assert_eq!(rows[1].tree_prefix, "└─ ");
        assert_eq!(app.table_state.selected(), Some(1));
        assert_eq!(
            app.current_field().unwrap().path,
            path(&["browser_use", "allow_history_access"])
        );
        assert_eq!(rows[1].value.as_deref(), original.map(|_| "false"));

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
                assert!(screen.contains("└─ allow_history_access"), "{screen}");
                assert!(
                    screen.contains("キー: browser_use.allow_history_access"),
                    "{screen}"
                );
            }
        }
        assert!(!app.config().unwrap().dirty());
        if let Some(original) = original {
            app.handle_key(key(KeyCode::Char(' ')));
            let Parsed::Toml(document) = app.config().unwrap().parsed.as_ref().unwrap() else {
                panic!()
            };
            assert_eq!(
                document["browser_use"]["allow_history_access"].as_bool(),
                Some(true)
            );
            assert_eq!(document["browser_use"]["unknown"].as_integer(), Some(7));
            assert!(app.config().unwrap().text.contains("# keep"));
            app.query.clear();
            let index = app
                .rows()
                .iter()
                .position(|field| field.path == path(&["browser_use", "custom_toggle"]))
                .unwrap();
            app.table_state.select(Some(index));
            assert_eq!(app.current_field().unwrap().value.as_deref(), Some("true"));
            app.handle_key(key(KeyCode::Char(' ')));
            assert_eq!(app.current_field().unwrap().value.as_deref(), Some("false"));
            assert!(app.config().unwrap().text.contains("# extra"));
            app.modal = Some(Modal::Search(Editor::new(
                "browser_use.allow_history_access",
            )));
            app.handle_key(key(KeyCode::Enter));
            assert_eq!(app.current_field().unwrap().value.as_deref(), Some("true"));
            app.handle_key(key(KeyCode::Char(' ')));
            let Parsed::Toml(document) = app.config().unwrap().parsed.as_ref().unwrap() else {
                panic!()
            };
            assert_eq!(
                document["browser_use"]["allow_history_access"].as_bool(),
                Some(false)
            );
            assert_eq!(document["browser_use"]["unknown"].as_integer(), Some(7));
            assert!(app.config().unwrap().text.contains("# keep"));
            assert_eq!(
                fs::read_to_string(fixture.path(".codex/config.toml")).unwrap(),
                original
            );
        } else {
            assert!(!fixture.path(".codex").exists());
        }
    }
}

#[test]
fn codex_hierarchy_keeps_quoted_names_whole_and_uses_codex_group_labels() {
    let fixture = Fixture::new();
    fixture.write(".codex/config.toml", "[model_providers.\"work.api\"]\nbase_url = 'https://example.com/old'\n[mcp_servers.\"work.server\".oauth]\nclient_id = 'sample-client'\n");
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Char(']')));
    app.focus = Focus::Settings;
    let rows = app.rows();
    let group = rows
        .iter()
        .find(|field| field.path == path(&["mcp_servers", "work.server"]))
        .unwrap();
    assert!(group.group);
    assert!(group.label.starts_with("MCPサーバー"));
    assert!(group.disabled_reason().is_some());
    assert!(
        !rows
            .iter()
            .any(|field| field.path == path(&["model_providers", "work"]))
    );
    let index = rows
        .iter()
        .position(|field| field.path == path(&["model_providers", "work.api", "base_url"]))
        .unwrap();
    assert!(!rows[index].tree_prefix.is_empty());
    app.table_state.select(Some(index));
    app.handle_key(key(KeyCode::Enter));
    let Some(Modal::Edit {
        editor,
        path: Some(selected),
        ..
    }) = &mut app.modal
    else {
        panic!()
    };
    assert_eq!(
        selected,
        &path(&["model_providers", "work.api", "base_url"])
    );
    *editor = Editor::new("https://example.com/next");
    app.handle_key(control('s'));
    let Parsed::Toml(document) = app.config().unwrap().parsed.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(
        document["model_providers"]["work.api"]["base_url"].as_str(),
        Some("https://example.com/next")
    );
    assert_eq!(
        document["mcp_servers"]["work.server"]["oauth"]["client_id"].as_str(),
        Some("sample-client")
    );
    assert!(document["model_providers"].get("work").is_none());
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
    // profile は公式の設定表の外 (関連ページと公開版のソース) で廃止を確かめた項目。
    fixture.write(".codex/config.toml", "model = 'sample-model'\nprofile = 'work'\n[features]\npersonality = false\n[features.guardianv2]\nasync_classifier_mode = 'conversations'\nthread_context = false\n[features.multi_agent_v2]\nusage_hint_enabled = false\n");
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
fn workspace_root_permissions_use_quoted_template_choices_and_save_one_value() {
    let fixture = Fixture::new();
    let original =
        "[permissions.work.filesystem.\":workspace_roots\"]\n\"docs/**\" = \"read\" # 読むだけ\n";
    let file = fixture.write(".codex/config.toml", original);
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Char(']')));
    // 引用された部品 (:workspace_roots) を1つの名前のままテンプレートと照合する。
    select_field(
        &mut app,
        "permissions.work.filesystem.\":workspace_roots\".\"docs/**\"",
    );
    let Some(Modal::Select { options, .. }) = &app.modal else {
        panic!("候補の選択画面が開いていません");
    };
    let labels = options
        .iter()
        .map(|option| option.label.as_str())
        .collect::<Vec<_>>();
    assert!(
        ["read", "write", "deny"]
            .iter()
            .all(|value| labels.contains(value)),
        "{labels:?}"
    );
    let write = options
        .iter()
        .find(|option| option.label == "write")
        .unwrap();
    assert!(write.help.description.contains("ワークスペースルート"));
    assert!(
        write
            .help
            .source
            .starts_with("https://learn.chatgpt.com/docs/permissions")
    );
    choose(&mut app, "write");
    app.config_mut().unwrap().save().unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        original.replace("\"read\"", "\"write\"")
    );
}

#[test]
fn confirmed_statuses_show_reason_and_source_and_retired_values_can_be_removed() {
    let fixture = Fixture::new();
    fixture.write(
        ".claude/settings.json",
        r#"{"subagentStatusLine":{"type":"command","command":"status"}}"#,
    );
    let original = "profile = 'work'\n[features]\ncodex_hooks = true\n";
    let file = fixture.write(".codex/config.toml", original);
    let mut app = fixture.app();
    app.focus = Focus::Settings;
    // 資料から消えた設定も、更新履歴で確かめた廃止として理由と出典を示す。
    let index = app
        .rows()
        .iter()
        .position(|field| field.key() == "subagentStatusLine")
        .unwrap();
    app.table_state.select(Some(index));
    let field = app.current_field().unwrap();
    assert!(field.retired);
    assert!(field.status_reason.unwrap().contains("2.1.323"));
    let mut terminal = Terminal::new(TestBackend::new(200, 50)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let screen = screen_text(&terminal);
    assert!(screen.contains("状態の出典"), "{screen}");
    assert!(screen.contains("2.1.323"), "{screen}");

    app.handle_key(key(KeyCode::Char(']')));
    app.focus = Focus::Settings;
    let rows = app.rows();
    let hooks = rows
        .iter()
        .find(|field| field.key() == "features.codex_hooks")
        .unwrap();
    assert!(hooks.deprecated);
    assert!(hooks.status_reason.unwrap().contains("features.hooks"));
    // 起動エラーになる廃止の設定は編集できないが、dで削除はできる。
    let index = rows
        .iter()
        .position(|field| field.key() == "profile")
        .unwrap();
    assert!(rows[index].retired);
    assert!(rows[index].disabled_reason().is_some());
    app.table_state.select(Some(index));
    app.handle_key(key(KeyCode::Char('d')));
    app.handle_key(key(KeyCode::Char('y')));
    app.config_mut().unwrap().save().unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        original.replace("profile = 'work'\n", "")
    );
    assert!(!app.rows().iter().any(|field| field.key() == "profile"));
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

    // 項目の描画色を確かめる際、検索タイトルにキーが出ないようにする。
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

#[test]
fn arrays_of_tables_open_the_whole_document_editor_without_inlining() {
    // テーブルの配列は、カタログやテンプレートの型が構造化された値でも全体編集で扱う。
    // 変えずに反映しても、インラインの配列に書き直してコメントを失わない。
    let fixture = Fixture::new();
    let original = "# skills\n[[skills.config]]\n# first\npath = \"/a/SKILL.md\"\nenabled = false\n\n[[skills.config]]\npath = \"/b/SKILL.md\"\nenabled = true\n\n# guard\n[[hooks.ExampleEvent]]\nmatcher = \"shell\"\n";
    fixture.write(".codex/config.toml", original);
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Char(']')));
    let rows = app.rows();
    // 任意のイベント名の項目は、カタログのテンプレートから作られる。
    assert!(
        rows.iter()
            .find(|field| field.key() == "hooks.ExampleEvent")
            .unwrap()
            .common
    );
    for name in ["skills.config", "hooks.ExampleEvent"] {
        let field = rows.iter().find(|field| field.key() == name).unwrap();
        assert_eq!(field.kind, fields::Kind::Document, "{name}");
        select_field(&mut app, name);
        assert!(
            matches!(&app.modal, Some(Modal::Edit { path: None, .. })),
            "{name}"
        );
        app.handle_key(control('s'));
        assert!(app.modal.is_none());
        assert_eq!(app.config().unwrap().text, original);
        assert_eq!(app.dirty_count(), 0);
    }
}

#[test]
fn raw_values_holding_strings_open_with_their_literal_and_apply_unchanged() {
    // 文字列も配列も取る項目では、文字列の値を元の引用符の表記で入力欄に出す。
    // 変えずに反映しても数値に変えたり、構文の誤りにしたりしない。
    let fixture = Fixture::new();
    let json = "{\n  \"forceLoginOrgUUID\": \"123e4567-e89b-12d3-a456-426614174000\"\n}\n";
    let toml = "chatgpt_base_url = 'https://chatgpt.example/backend-api/'\n\n[tui.keymap.vim_normal]\nmove_line_start = '0' # keep\n";
    fixture.write(".claude/settings.json", json);
    fixture.write(".codex/config.toml", toml);
    let mut app = fixture.app();
    select_field(&mut app, "forceLoginOrgUUID");
    let Some(Modal::Edit { editor, plain, .. }) = &app.modal else {
        panic!()
    };
    assert!(!plain);
    assert_eq!(editor.text(), "\"123e4567-e89b-12d3-a456-426614174000\"");
    app.handle_key(control('s'));
    assert!(app.modal.is_none());
    assert_eq!(app.config().unwrap().text, json);
    app.handle_key(key(KeyCode::Char(']')));
    select_field(&mut app, "tui.keymap.vim_normal.move_line_start");
    let Some(Modal::Edit { editor, plain, .. }) = &app.modal else {
        panic!()
    };
    assert!(!plain);
    assert_eq!(editor.text(), "'0'");
    app.handle_key(control('s'));
    assert!(app.modal.is_none());
    assert_eq!(app.config().unwrap().text, toml);
    assert_eq!(app.dirty_count(), 0);
    // 文字列だけの項目は、これまでどおり引用符なしで編集する。
    select_field(&mut app, "chatgpt_base_url");
    let Some(Modal::Edit { editor, plain, .. }) = &app.modal else {
        panic!()
    };
    assert!(plain);
    assert_eq!(editor.text(), "https://chatgpt.example/backend-api/");
}

#[test]
fn pasted_cr_and_crlf_line_breaks_start_new_lines() {
    // 端末によっては貼り付けの改行がCRだけやCRLFで届く。複数行の入力欄では改行にし、
    // 1行の入力欄ではこれまでどおり取り除く。
    let mut editor = Editor::new("");
    editor.insert(
        "# fast model\rmodel = \"gpt-5-mini\"\r\napproval_policy = \"never\"\r",
        true,
    );
    assert_eq!(
        editor.lines,
        [
            "# fast model",
            "model = \"gpt-5-mini\"",
            "approval_policy = \"never\"",
            ""
        ]
    );
    let Parsed::Toml(document) = parse(Provider::Codex, &editor.text()).unwrap() else {
        panic!()
    };
    assert_eq!(document["model"].as_str(), Some("gpt-5-mini"));
    let mut editor = Editor::new("");
    editor.insert("a\rb\r\nc", false);
    assert_eq!(editor.text(), "abc");
}

#[test]
fn unchanged_toml_strings_keep_their_quotes_and_escapes_and_stay_clean() {
    // 同じ文字列を反映しても、リテラル文字列・エスケープ・\u表記を書き換えず、
    // 変更や取り消しの履歴として数えない。
    for (original, name, value) in [
        (
            "chatgpt_base_url = 'https://chatgpt.example/backend-api/' # proxy\n",
            "chatgpt_base_url",
            "https://chatgpt.example/backend-api/",
        ),
        (
            "model_instructions_file = \"C:\\\\Users\\\\me\\\\AGENTS.md\"\n",
            "model_instructions_file",
            "C:\\Users\\me\\AGENTS.md",
        ),
        (
            "developer_instructions = \"caf\\u00e9\"\n",
            "developer_instructions",
            "café",
        ),
    ] {
        let fixture = Fixture::new();
        let mut config = fixture.config(Provider::Codex, original);
        config.set(&path(&[name]), value, true).unwrap();
        // 構造化された値の入力欄で別の引用符にしても、同じ内容なら書き換えない。
        let quoted = serde_json::to_string(value).unwrap();
        config.set(&path(&[name]), &quoted, false).unwrap();
        assert_eq!(config.text, original);
        assert!(!config.dirty());
        assert!(!config.undo());
    }
    let fixture = Fixture::new();
    let mut config = fixture.config(
        Provider::Codex,
        "chatgpt_base_url = 'https://chatgpt.example/backend-api/' # proxy\n",
    );
    config
        .set(&path(&["chatgpt_base_url"]), "https://other.example/", true)
        .unwrap();
    assert_eq!(
        config.text,
        "chatgpt_base_url = \"https://other.example/\" # proxy\n"
    );
}

#[test]
fn removing_tables_keeps_comments_above_array_and_child_table_headers() {
    // テーブルの配列や、子テーブルだけを持つ暗黙のテーブルを未設定に戻しても、
    // 見出しの上と行末のコメントは文書の末尾に残す。
    for (original, parts, comments, removed) in [
        (
            "model = \"gpt-5\"\n\n# keep: why these skills are disabled\n[[skills.config]] # first entry\npath = \"/a/SKILL.md\"\nenabled = false\n\n# second skill\n[[skills.config]]\npath = \"/b/SKILL.md\"\n",
            vec!["skills", "config"],
            vec![
                "# keep: why these skills are disabled",
                "# first entry",
                "# second skill",
            ],
            "SKILL.md",
        ),
        (
            "model = \"gpt-5\"\n\n# keep: github server for reviews\n[mcp_servers.github]\ncommand = \"gh-mcp\"\n",
            vec!["mcp_servers"],
            vec!["# keep: github server for reviews"],
            "gh-mcp",
        ),
    ] {
        let fixture = Fixture::new();
        let mut config = fixture.config(Provider::Codex, original);
        config.remove(&path(&parts)).unwrap();
        for comment in comments {
            assert!(config.text.contains(comment), "{}", config.text);
        }
        assert!(!config.text.contains(removed), "{}", config.text);
        assert!(config.text.starts_with("model = \"gpt-5\"\n"));
        assert!(parse(Provider::Codex, &config.text).is_ok());
        assert!(config.undo());
        assert_eq!(config.text, original);
    }
}

#[test]
fn editor_accepts_altgr_characters_and_keeps_control_shortcuts() {
    // WindowsではAltGrの文字がCtrl+Alt付きで届くため、文字として入力する。
    let altgr = |character| {
        KeyEvent::new(
            KeyCode::Char(character),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        )
    };
    let mut editor = Editor::new("");
    for character in ['{', '[', '@', '\\', '|', '~', '}', ']'] {
        editor.handle(altgr(character), true);
    }
    assert_eq!(editor.text(), "{[@\\|~}]");
    // Ctrlだけのショートカットはこれまでどおり動き、Altだけの文字は入力しない。
    editor.handle(control('a'), true);
    editor.handle(altgr('€'), true);
    assert_eq!(editor.text(), "€{[@\\|~}]");
    editor.handle(control('k'), true);
    assert_eq!(editor.text(), "€");
    editor.handle(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT), true);
    assert_eq!(editor.text(), "€");
    editor.handle(control('u'), true);
    assert_eq!(editor.text(), "");
}

#[test]
fn pasting_a_long_single_line_inserts_every_character_at_the_cursor() {
    // 10万文字の1行を貼り付けても、文字ごとに挿入位置を探し直さずにまとめて挿入する。
    // タブは空白2つにし、制御文字は入れない。
    let pasted = format!("{}\t{}\u{1b}", "x".repeat(50_000), "日本".repeat(25_000));
    let expected = format!("{}  {}", "x".repeat(50_000), "日本".repeat(25_000));
    let mut editor = Editor::new("{}");
    editor.handle(key(KeyCode::Left), true);
    editor.insert(&pasted, true);
    assert_eq!(editor.text(), format!("{{{expected}}}"));
    assert_eq!(editor.col, 1 + expected.chars().count());
    editor.handle(key(KeyCode::Char('!')), true);
    assert!(editor.text().ends_with("日本!}"));
}

#[test]
fn sensitive_values_outside_the_choices_stay_hidden_in_the_picker() {
    // 候補外の現在の値も、一覧と同じく表示を許可するまで伏せる。伏せても値は保持する。
    let fixture = Fixture::new();
    let original =
        "[mcp_servers.work.oauth]\ntoken_endpoint_auth_method = 'example-private-value'\n";
    fixture.write(".codex/config.toml", original);
    let mut app = fixture.app();
    app.handle_key(key(KeyCode::Char(']')));
    let name = "mcp_servers.work.oauth.token_endpoint_auth_method";
    select_field(&mut app, name);
    let mut terminal = Terminal::new(TestBackend::new(160, 45)).unwrap();
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    let screen = screen_text(&terminal);
    assert!(screen.contains("現在の値: [hidden]（候補外）"), "{screen}");
    assert!(!screen.contains("example-private-value"), "{screen}");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.config().unwrap().text, original);
    assert_eq!(app.dirty_count(), 0);
    app.handle_key(key(KeyCode::Char('h')));
    select_field(&mut app, name);
    terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
    assert!(screen_text(&terminal).contains("現在の値: example-private-value（候補外）"));
}
