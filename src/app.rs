use std::{path::PathBuf, time::Duration};

use anyhow::{Result, bail};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
};
use ratatui::{
    DefaultTerminal,
    widgets::{ListState, TableState},
};

use crate::{
    document::Config,
    editor::Editor,
    fields::{self, Field, Kind},
    profile::{Profiles, Provider, expand_path},
    ui,
};

pub enum Loaded {
    Pending,
    Ready(Box<Config>),
    Failed(String),
}
#[derive(Clone, Copy, PartialEq)]
pub enum Focus {
    Profiles,
    Settings,
}

pub enum Modal {
    Edit {
        editor: Editor,
        path: Option<Vec<String>>,
        plain: bool,
        title: String,
        error: Option<String>,
    },
    Add {
        provider: Provider,
        name: Editor,
        directory: Editor,
        active: usize,
        error: Option<String>,
    },
    Search(Editor),
    Confirm {
        action: Action,
        scroll: u16,
    },
    Help,
}

pub enum Action {
    Save,
    Reset(Vec<String>),
    Reload,
    RemoveProfile,
    Quit,
}

pub struct App {
    pub profiles: Profiles,
    pub documents: Vec<Loaded>,
    pub home: PathBuf,
    pub selected: usize,
    pub focus: Focus,
    pub profile_state: ListState,
    pub table_state: TableState,
    pub query: String,
    pub modal: Option<Modal>,
    pub status: String,
    pub is_error: bool,
    pub reveal: bool,
    pub quit: bool,
}

impl App {
    pub fn new(profiles: Profiles, home: PathBuf, initial: Option<&str>) -> Result<Self> {
        let selected = if let Some(name) = initial {
            profiles
                .items
                .iter()
                .position(|item| item.name == name)
                .ok_or_else(|| anyhow::anyhow!("指定されたプロファイルが見つかりません"))?
        } else {
            0
        };
        let documents = profiles.items.iter().map(|_| Loaded::Pending).collect();
        let mut app = Self {
            profiles,
            documents,
            home,
            selected,
            focus: Focus::Settings,
            profile_state: ListState::default().with_selected(Some(selected)),
            table_state: TableState::default().with_selected(Some(0)),
            query: String::new(),
            modal: None,
            status: "Enter: 編集  /  Ctrl+S: 保存  /  a: プロファイル追加  /  ?: ヘルプ".into(),
            is_error: false,
            reveal: false,
            quit: false,
        };
        app.ensure_loaded();
        Ok(app)
    }

    pub fn config(&self) -> Option<&Config> {
        match &self.documents[self.selected] {
            Loaded::Ready(config) => Some(config),
            _ => None,
        }
    }
    pub fn config_mut(&mut self) -> Option<&mut Config> {
        match &mut self.documents[self.selected] {
            Loaded::Ready(config) => Some(config),
            _ => None,
        }
    }
    pub fn rows(&self) -> Vec<Field> {
        let query = self.query.to_lowercase();
        self.config()
            .map(fields::fields)
            .unwrap_or_default()
            .into_iter()
            .filter(|field| {
                query.is_empty()
                    || format!("{} {} {}", field.key(), field.label, field.description)
                        .to_lowercase()
                        .contains(&query)
            })
            .collect()
    }
    pub fn current_field(&self) -> Option<Field> {
        self.rows()
            .get(self.table_state.selected().unwrap_or(0))
            .cloned()
    }
    pub fn dirty_count(&self) -> usize {
        self.documents
            .iter()
            .filter(|state| matches!(state, Loaded::Ready(config) if config.dirty()))
            .count()
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        execute!(std::io::stdout(), event::EnableBracketedPaste)?;
        let result = self.event_loop(terminal);
        let cleanup = execute!(std::io::stdout(), event::DisableBracketedPaste);
        result.and(cleanup.map_err(Into::into))
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            terminal.draw(|frame| ui::draw(frame, self))?;
            if event::poll(Duration::from_millis(200))? {
                match event::read()? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => self.handle_key(key),
                    Event::Paste(text) => self.paste(&text),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.modal.is_some() {
            self.handle_modal(key);
            return;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('c')
                if key.code == KeyCode::Char('q') || control =>
            {
                if self.dirty_count() > 0 {
                    self.confirm(Action::Quit);
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Char('s') => self.request_save(),
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = if self.focus == Focus::Profiles {
                    Focus::Settings
                } else {
                    Focus::Profiles
                }
            }
            KeyCode::Left => self.focus = Focus::Profiles,
            KeyCode::Right => self.focus = Focus::Settings,
            KeyCode::Up | KeyCode::Char('k') => self.navigate(-1),
            KeyCode::Down | KeyCode::Char('j') => self.navigate(1),
            KeyCode::PageUp => self.navigate(-10),
            KeyCode::PageDown => self.navigate(10),
            KeyCode::Char('[') => self.select_profile(self.selected.saturating_sub(1)),
            KeyCode::Char(']') => {
                self.select_profile((self.selected + 1).min(self.profiles.items.len() - 1))
            }
            KeyCode::Enter | KeyCode::Char('e') if self.focus == Focus::Settings => {
                self.edit_field()
            }
            KeyCode::Enter if self.focus == Focus::Profiles => self.focus = Focus::Settings,
            KeyCode::Char(' ') if self.focus == Focus::Settings => self.toggle_bool(),
            KeyCode::Char('E') | KeyCode::F(4) => self.edit_document(),
            KeyCode::Char('a') => {
                self.modal = Some(Modal::Add {
                    provider: Provider::Claude,
                    name: Editor::new(""),
                    directory: Editor::new("~/"),
                    active: 0,
                    error: None,
                })
            }
            KeyCode::Char('x') if self.focus == Focus::Profiles => {
                if self.selected < 2 {
                    self.message("既定のプロファイルは解除できません", true);
                } else {
                    self.confirm(Action::RemoveProfile);
                }
            }
            KeyCode::Char('d') if self.focus == Focus::Settings => {
                if let Some(field) = self.current_field().filter(|field| field.value.is_some()) {
                    self.confirm(Action::Reset(field.path));
                }
            }
            KeyCode::Char('r') => {
                if self.config().is_some_and(Config::dirty) {
                    self.confirm(Action::Reload);
                } else {
                    self.reload();
                }
            }
            KeyCode::Char('/') => self.modal = Some(Modal::Search(Editor::new(&self.query))),
            KeyCode::Esc => {
                self.query.clear();
                self.table_state.select(Some(0));
            }
            KeyCode::Char('h') => self.reveal = !self.reveal,
            KeyCode::Char('u') => {
                let undone = self.config_mut().is_some_and(Config::undo);
                self.clamp_selection();
                self.message(
                    if undone {
                        "変更を1つ取り消しました"
                    } else {
                        "取り消す変更はありません"
                    },
                    false,
                );
            }
            KeyCode::Char('?') | KeyCode::F(1) => self.modal = Some(Modal::Help),
            _ => {}
        }
    }

    fn handle_modal(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.modal = None;
            return;
        }
        let Some(mut modal) = self.modal.take() else {
            return;
        };
        let apply = key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL);
        match &mut modal {
            Modal::Edit {
                editor,
                path,
                plain,
                error,
                ..
            } => {
                if key.code == KeyCode::Esc {
                    return;
                }
                if apply || (key.code == KeyCode::Enter && *plain) {
                    let result = if let Some(config) = self.config_mut() {
                        if let Some(path) = path {
                            config.set(path, &editor.text(), *plain)
                        } else {
                            config.replace(editor.text())
                        }
                    } else {
                        bail_result("設定ファイルを読み込めません")
                    };
                    match result {
                        Ok(()) => {
                            self.message("変更を反映しました。Ctrl+Sでファイルへ保存します", false);
                            self.clamp_selection();
                            return;
                        }
                        Err(problem) => *error = Some(format!("{problem:#}")),
                    }
                } else {
                    editor.handle(key, !*plain);
                }
            }
            Modal::Add {
                provider,
                name,
                directory,
                active,
                error,
            } => {
                if key.code == KeyCode::Esc {
                    return;
                }
                if apply {
                    let result =
                        expand_path(std::path::Path::new(directory.text().trim()), &self.home)
                            .and_then(|directory| {
                                self.profiles.add(name.text(), *provider, directory)
                            });
                    match result {
                        Ok(()) => {
                            self.documents.push(Loaded::Pending);
                            self.select_profile(self.profiles.items.len() - 1);
                            self.message("プロファイルを登録しました", false);
                            return;
                        }
                        Err(problem) => *error = Some(format!("{problem:#}")),
                    }
                } else {
                    match key.code {
                        KeyCode::Tab | KeyCode::Down | KeyCode::Enter => {
                            *active = (*active + 1) % 3
                        }
                        KeyCode::BackTab | KeyCode::Up => *active = (*active + 2) % 3,
                        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if *active == 0 => {
                            *provider = match provider {
                                Provider::Claude => Provider::Codex,
                                Provider::Codex => Provider::Claude,
                            }
                        }
                        _ if *active == 1 => name.handle(key, false),
                        _ if *active == 2 => directory.handle(key, false),
                        _ => {}
                    }
                }
            }
            Modal::Search(editor) => {
                if key.code == KeyCode::Esc {
                    return;
                }
                if key.code == KeyCode::Enter {
                    self.query = editor.text();
                    self.table_state.select(Some(0));
                    return;
                }
                editor.handle(key, false);
            }
            Modal::Help => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') | KeyCode::Char('q')
                ) {
                    return;
                }
            }
            Modal::Confirm { action, scroll } => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('n')) {
                    return;
                }
                if matches!(
                    key.code,
                    KeyCode::Down | KeyCode::PageDown | KeyCode::Char('j')
                ) {
                    *scroll = scroll.saturating_add(5);
                }
                if matches!(key.code, KeyCode::Up | KeyCode::PageUp | KeyCode::Char('k')) {
                    *scroll = scroll.saturating_sub(5);
                }
                if matches!(key.code, KeyCode::Enter | KeyCode::Char('y')) {
                    let result = self.perform(action);
                    if let Err(problem) = result {
                        self.message(format!("{problem:#}"), true);
                    }
                    return;
                }
            }
        }
        self.modal = Some(modal);
    }

    fn perform(&mut self, action: &Action) -> Result<()> {
        match action {
            Action::Save => {
                if let Some(config) = self.config_mut() {
                    let backup = config.save()?;
                    self.message(
                        backup.map_or_else(
                            || "保存しました".into(),
                            |path| format!("保存しました · バックアップ: {}", path.display()),
                        ),
                        false,
                    );
                }
            }
            Action::Reset(path) => {
                if let Some(config) = self.config_mut() {
                    config.remove(path)?;
                }
                self.message("設定を未設定に戻しました。Ctrl+Sで保存します", false);
                self.clamp_selection();
            }
            Action::Reload => self.reload(),
            Action::RemoveProfile => {
                self.profiles.remove(self.selected)?;
                self.documents.remove(self.selected);
                self.select_profile(self.selected.min(self.profiles.items.len() - 1));
                self.message("登録を解除しました（設定ファイルは保持されます）", false);
            }
            Action::Quit => self.quit = true,
        }
        Ok(())
    }

    fn request_save(&mut self) {
        if self.config().is_some_and(Config::dirty) {
            self.confirm(Action::Save);
        } else {
            self.message("保存する変更はありません", false);
        }
    }
    fn confirm(&mut self, action: Action) {
        self.modal = Some(Modal::Confirm { action, scroll: 0 });
    }
    fn edit_field(&mut self) {
        let Some(field) = self.current_field() else {
            return;
        };
        if field.kind == Kind::Document {
            self.edit_document();
            return;
        }
        let text = field.value.clone().unwrap_or_else(|| match field.kind {
            Kind::Bool => "false".into(),
            Kind::Raw if self.profiles.items[self.selected].provider == Provider::Claude => {
                if field.key().starts_with("permissions.") {
                    "[]".into()
                } else {
                    "{}".into()
                }
            }
            Kind::Raw => "true".into(),
            _ => String::new(),
        });
        self.modal = Some(Modal::Edit {
            editor: Editor::new(&text),
            path: Some(field.path),
            plain: field.kind == Kind::String,
            title: format!("{} · {}", field.label, field.suggestions),
            error: None,
        });
    }
    fn edit_document(&mut self) {
        if let Some(config) = self.config() {
            self.modal = Some(Modal::Edit {
                editor: Editor::new(&config.text),
                path: None,
                plain: false,
                title: format!("{} 全体編集", config.provider.filename()),
                error: None,
            });
        }
    }
    fn toggle_bool(&mut self) {
        let Some(field) = self
            .current_field()
            .filter(|field| field.kind == Kind::Bool)
        else {
            return;
        };
        if let Some(config) = self.config_mut() {
            let result = config.set(
                &field.path,
                if field.value.as_deref() == Some("true") {
                    "false"
                } else {
                    "true"
                },
                false,
            );
            match result {
                Ok(()) => self.message("値を切り替えました。Ctrl+Sで保存します", false),
                Err(error) => self.message(format!("{error:#}"), true),
            }
        }
    }
    fn paste(&mut self, text: &str) {
        match &mut self.modal {
            Some(Modal::Edit { editor, plain, .. }) => editor.insert(text, !*plain),
            Some(Modal::Add { name, active, .. }) if *active == 1 => name.insert(text, false),
            Some(Modal::Add {
                directory, active, ..
            }) if *active == 2 => directory.insert(text, false),
            Some(Modal::Search(editor)) => editor.insert(text, false),
            _ => {}
        }
    }
    fn navigate(&mut self, offset: isize) {
        if self.focus == Focus::Profiles {
            self.select_profile(
                self.selected
                    .saturating_add_signed(offset)
                    .min(self.profiles.items.len() - 1),
            );
        } else {
            self.table_state.select(Some(
                self.table_state
                    .selected()
                    .unwrap_or(0)
                    .saturating_add_signed(offset)
                    .min(self.rows().len().saturating_sub(1)),
            ));
        }
    }
    fn select_profile(&mut self, index: usize) {
        self.selected = index;
        self.profile_state.select(Some(index));
        self.table_state.select(Some(0));
        self.query.clear();
        self.reveal = false;
        self.ensure_loaded();
    }
    fn ensure_loaded(&mut self) {
        if matches!(self.documents[self.selected], Loaded::Pending) {
            let profile = &self.profiles.items[self.selected];
            self.documents[self.selected] = match Config::load(
                profile.path(),
                profile.provider,
                self.profiles.state_dir.clone(),
            ) {
                Ok(config) => Loaded::Ready(Box::new(config)),
                Err(error) => Loaded::Failed(format!("{error:#}")),
            };
        }
    }
    fn reload(&mut self) {
        self.documents[self.selected] = Loaded::Pending;
        self.ensure_loaded();
        self.clamp_selection();
        self.message("ファイルを再読み込みしました", false);
    }
    fn clamp_selection(&mut self) {
        let index = self
            .table_state
            .selected()
            .unwrap_or(0)
            .min(self.rows().len().saturating_sub(1));
        self.table_state.select(Some(index));
    }
    fn message(&mut self, message: impl Into<String>, error: bool) {
        self.status = message.into();
        self.is_error = error;
    }
}

fn bail_result(message: &str) -> Result<()> {
    bail!("{message}")
}
