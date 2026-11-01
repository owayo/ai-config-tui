use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap,
    },
};
use similar::{ChangeTag, TextDiff};

use crate::{
    app::{Action, App, Focus, Loaded, Modal},
    catalog::CATALOG,
    editor::Editor,
    fields::Kind,
    profile::{Provider, expand_path},
};

const BG: Color = Color::Rgb(17, 23, 31);
const PANEL: Color = Color::Rgb(23, 31, 41);
const BORDER: Color = Color::Rgb(55, 69, 83);
const TEXT: Color = Color::Rgb(218, 228, 237);
const MUTED: Color = Color::Rgb(137, 155, 172);
const ACCENT: Color = Color::Rgb(105, 219, 195);
const AMBER: Color = Color::Rgb(245, 194, 115);
const RED: Color = Color::Rgb(248, 134, 141);
const SELECTED: Color = Color::Rgb(35, 64, 70);

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(BG).fg(TEXT)),
        area,
    );
    if area.width < 60 || area.height < 16 {
        frame.render_widget(
            Paragraph::new(
                "端末を60列 × 16行以上に広げてください\nq: 終了（未保存の変更があれば確認）",
            )
            .wrap(Wrap { trim: false }),
            area,
        );
        if app.modal.is_some() {
            modal(frame, app);
        }
        return;
    }
    let regions = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(6),
        Constraint::Length(2),
        Constraint::Length(2),
    ])
    .split(area);
    let header = Line::from(vec![
        Span::styled(
            "  AI CONFIG",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  /  Claude Code + Codex", Style::default().fg(MUTED)),
        Span::styled(
            format!(
                "    {} profiles  ·  {} unsaved",
                app.profiles.items.len(),
                app.dirty_count()
            ),
            Style::default().fg(AMBER),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(header).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(BORDER)),
        ),
        regions[0],
    );
    let sidebar_width = (regions[1].width / 4).clamp(20, 34);
    let body = Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(30)])
        .split(regions[1]);
    profiles(frame, app, body[0]);
    content(frame, app, body[1]);
    frame.render_widget(
        Paragraph::new(safe(&app.status))
            .style(Style::default().fg(if app.is_error { RED } else { ACCENT }))
            .wrap(Wrap { trim: false }),
        regions[2],
    );
    frame.render_widget(Paragraph::new(" Tab ペイン  ↑↓ 移動  Enter 編集  Space 切替  Ctrl+S 保存  E 全体編集\n a 追加  / 検索  d 未設定  r 再読込  [ ] プロファイル  ? ヘルプ  q 終了")
        .style(Style::default().fg(MUTED)), regions[3]);
    if app.modal.is_some() {
        modal(frame, app);
    }
}

fn panel(title: impl Into<String>, active: bool) -> Block<'static> {
    Block::default()
        .title(format!(" {} ", title.into()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if active { ACCENT } else { BORDER }))
        .style(Style::default().bg(PANEL).fg(TEXT))
}

fn profiles(frame: &mut Frame, app: &mut App, area: Rect) {
    let items = app
        .profiles
        .display_order()
        .into_iter()
        .map(|index| {
            let profile = &app.profiles.items[index];
            let dirty = matches!(&app.documents[index], Loaded::Ready(config) if config.dirty());
            let color = if dirty { AMBER } else { TEXT };
            ListItem::new(vec![
                Line::from(Span::styled(
                    format!("{}{}", safe(&profile.name), if dirty { " *" } else { "" }),
                    Style::default().fg(color),
                )),
                Line::from(Span::styled(
                    format!(
                        "{} · {}",
                        profile.provider.label(),
                        short_path(&profile.directory, app)
                    ),
                    Style::default().fg(MUTED),
                )),
                Line::from(""),
            ])
        })
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(panel("PROFILES", app.focus == Focus::Profiles))
        .highlight_style(Style::default().bg(SELECTED))
        .highlight_symbol("▌ ");
    frame.render_stateful_widget(list, area, &mut app.profile_state);
}

fn content(frame: &mut Frame, app: &mut App, area: Rect) {
    let profile = &app.profiles.items[app.selected];
    if let Loaded::Failed(error) = &app.documents[app.selected] {
        frame.render_widget(
            Paragraph::new(format!(
                "\n{}\n\n{}\n\nr: 再読み込み  /  a: 別のプロファイルを追加",
                short_path(&profile.path(), app),
                safe(error)
            ))
            .wrap(Wrap { trim: false })
            .block(panel("読み込みエラー", true))
            .style(Style::default().fg(RED)),
            area,
        );
        return;
    }
    if let Some(error) = app.config().and_then(|config| config.error.as_deref()) {
        frame.render_widget(
            Paragraph::new(format!(
                "\n{}\n\nE: 全体編集で修正  /  r: 再読み込み\n\n元のファイルは保持されています。",
                safe(error)
            ))
            .wrap(Wrap { trim: false })
            .block(panel(profile.provider.filename(), true))
            .style(Style::default().fg(RED)),
            area,
        );
        return;
    }
    let (settings_area, detail_area) = if area.width >= 90 {
        let layout = Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);
        (layout[0], layout[1])
    } else {
        let layout = Layout::vertical([Constraint::Min(4), Constraint::Length(8)]).split(area);
        (layout[0], layout[1])
    };
    let rows = app.rows();
    let table_rows = rows.iter().map(|field| {
        let value = if !app.reveal && secret(&field.key()) && field.value.is_some() {
            "[hidden]".into()
        } else {
            safe(&field.display_value())
        };
        let key = if field.common {
            field.key()
        } else {
            format!("{} +", field.key())
        };
        Row::new(vec![
            Cell::from(Text::from(vec![
                Line::from(key),
                Line::from(Span::styled(
                    field.label.clone(),
                    Style::default().fg(MUTED),
                )),
            ])),
            Cell::from(value).style(Style::default().fg(if field.value.is_some() {
                ACCENT
            } else {
                MUTED
            })),
        ])
        .height(2)
    });
    let title = if app.query.is_empty() {
        format!(
            "{} · {} · {}項目",
            profile.provider.label(),
            profile.provider.format(),
            rows.len()
        )
    } else {
        format!("検索: {} · {}件", safe(&app.query), rows.len())
    };
    let table = Table::new(
        table_rows,
        [Constraint::Percentage(61), Constraint::Percentage(39)],
    )
    .header(
        Row::new(["設定 / キー", "値"])
            .style(Style::default().fg(MUTED))
            .bottom_margin(1),
    )
    .block(panel(title, app.focus == Focus::Settings))
    .row_highlight_style(Style::default().bg(SELECTED))
    .highlight_symbol("› ")
    .column_spacing(1);
    frame.render_stateful_widget(table, settings_area, &mut app.table_state);
    let mut detail = vec![
        Line::from(Span::styled(
            short_path(&profile.path(), app),
            Style::default().fg(MUTED),
        )),
        Line::from(""),
    ];
    if let Some(field) = app.current_field() {
        detail.push(Line::from(Span::styled(
            field.label.clone(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )));
        detail.push(Line::from(safe(&field.description)));
        detail.push(Line::from(""));
        detail.push(Line::from(Span::styled(
            match field.kind {
                Kind::String => "文字列 · 引用符不要",
                Kind::Bool => "真偽値 · Spaceで切替",
                Kind::Raw => "構造化された値 · Enterで編集",
                Kind::Document => "全体編集で変更",
            },
            Style::default().fg(AMBER),
        )));
        if !field.suggestions.is_empty() {
            detail.push(Line::from(safe(&field.suggestions)));
        }
        if !field.choices.is_empty() {
            detail.push(Line::from(Span::styled(
                "Enter: 候補から選択",
                Style::default().fg(ACCENT),
            )));
        }
        if let Some(reason) = field.disabled_reason() {
            detail.push(Line::from(Span::styled(
                safe(reason),
                Style::default().fg(AMBER),
            )));
        }
        detail.push(Line::from(""));
        let text = if !app.reveal && secret(&field.path.join(".")) {
            "[hidden] · hで表示".into()
        } else {
            field
                .value
                .unwrap_or_else(|| "未設定 · ファイルには書き込みません".into())
        };
        detail.extend(safe(&text).lines().map(|line| Line::from(line.to_owned())));
        if !field.default_value.is_empty() {
            detail.push(Line::from(format!(
                "公式の既定値: {}",
                safe(&field.default_value)
            )));
        }
        if !field.source.is_empty() {
            detail.push(Line::from(format!("適用範囲: {}", safe(&field.scope))));
            detail.push(Line::from(format!("参照日: {}", CATALOG.verified_at)));
            detail.push(Line::from(Span::styled(
                safe(&field.source),
                Style::default().fg(MUTED),
            )));
        }
    } else {
        detail.push(Line::from("該当する設定がありません。Escで検索を解除。"));
    }
    if app.config().is_some_and(|config| !config.snapshot.exists()) {
        detail.extend([
            Line::from(""),
            Line::from(Span::styled(
                "保存時にファイルを新規作成します",
                Style::default().fg(AMBER),
            )),
        ]);
    }
    frame.render_widget(
        Paragraph::new(detail)
            .wrap(Wrap { trim: false })
            .block(panel("DETAIL", false)),
        detail_area,
    );
}

fn modal(frame: &mut Frame, app: &mut App) {
    let Some(mut modal) = app.modal.take() else {
        return;
    };
    match &mut modal {
        Modal::Select {
            field,
            options,
            state,
            error,
        } => {
            let height =
                (options.len() + 8).min(frame.area().height.saturating_sub(2) as usize) as u16;
            let area = popup(frame.area(), 80, height);
            frame.render_widget(Clear, area);
            frame.render_widget(
                panel(format!("{} · 候補から選択", safe(&field.label)), true),
                area,
            );
            let parts = Layout::vertical([
                Constraint::Length(2),
                Constraint::Min(1),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(area.inner(ratatui::layout::Margin::new(2, 1)));
            frame.render_widget(
                Paragraph::new(format!(
                    "{} · ↑↓で選び、Enterで反映\n選択した設定はCtrl+Sでファイルへ保存します。",
                    safe(&field.key())
                ))
                .style(Style::default().fg(MUTED)),
                parts[0],
            );
            let items = options
                .iter()
                .map(|option| ListItem::new(safe(&option.label)))
                .collect::<Vec<_>>();
            frame.render_stateful_widget(
                List::new(items)
                    .highlight_symbol("› ")
                    .highlight_style(Style::default().bg(SELECTED).fg(ACCENT)),
                parts[1],
                state,
            );
            if let Some(error) = error {
                frame.render_widget(
                    Paragraph::new(safe(error))
                        .style(Style::default().fg(RED))
                        .wrap(Wrap { trim: false }),
                    parts[2],
                );
            }
            frame.render_widget(
                Paragraph::new("Enter: 反映   Esc: キャンセル   Home/End: 先頭/末尾")
                    .style(Style::default().fg(AMBER)),
                parts[3],
            );
        }
        Modal::Edit {
            editor,
            title,
            error,
            plain,
            ..
        } => {
            let area = popup(
                frame.area(),
                92,
                if *plain {
                    11
                } else {
                    frame.area().height.saturating_sub(4)
                },
            );
            frame.render_widget(Clear, area);
            frame.render_widget(panel(safe(title), true), area);
            let inner = area.inner(ratatui::layout::Margin::new(2, 1));
            let parts = Layout::vertical([
                Constraint::Min(1),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(inner);
            draw_editor(frame, editor, parts[0]);
            if let Some(error) = error {
                frame.render_widget(
                    Paragraph::new(safe(error))
                        .style(Style::default().fg(RED))
                        .wrap(Wrap { trim: false }),
                    parts[1],
                );
            }
            frame.render_widget(
                Paragraph::new(if *plain {
                    "Enter / Ctrl+S: 変更を反映   Esc: キャンセル"
                } else {
                    "Ctrl+S: 変更を反映   Esc: キャンセル   Ctrl+Home/End: 先頭/末尾"
                })
                .style(Style::default().fg(MUTED)),
                parts[2],
            );
        }
        Modal::Add {
            provider,
            name,
            directory,
            active,
            error,
        } => {
            let area = popup(frame.area(), 78, 17);
            frame.render_widget(Clear, area);
            frame.render_widget(panel("プロファイルを追加", true), area);
            let parts = Layout::vertical([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area.inner(ratatui::layout::Margin::new(2, 1)));
            let directory_text = directory.text();
            let expanded = expand_path(std::path::Path::new(directory_text.trim()), &app.home);
            let detected = expanded
                .as_ref()
                .ok()
                .and_then(|path| Provider::detect(path).ok().flatten());
            let effective = provider.or(detected);
            let kind_label = match provider {
                Some(value) => format!("{}（手動）", value.label()),
                None => effective.map_or_else(
                    || "自動判定（未判定）".into(),
                    |value| format!("自動判定 → {}", value.label()),
                ),
            };
            frame.render_widget(
                Paragraph::new(format!("  ‹ {kind_label} ›   ← → / Spaceで切替"))
                    .block(panel("種類", *active == 0)),
                parts[0],
            );
            input(frame, name, parts[1], "名前（例: Work）", *active == 1);
            input(
                frame,
                directory,
                parts[2],
                "ディレクトリ（~ 使用可）",
                *active == 2,
            );
            if let Some(error) = error {
                frame.render_widget(
                    Paragraph::new(safe(error))
                        .style(Style::default().fg(RED))
                        .wrap(Wrap { trim: false }),
                    parts[3],
                );
            } else {
                let message = match (expanded, effective) {
                    (Ok(path), Some(value)) => format!("対象: {}", short_path(&path.join(value.filename()), app)),
                    _ => "ディレクトリから自動判定します。未判定の場合は種類を手動で選択してください。".into(),
                };
                frame.render_widget(
                    Paragraph::new(message)
                        .style(Style::default().fg(MUTED))
                        .wrap(Wrap { trim: false }),
                    parts[3],
                );
            }
            frame.render_widget(
                Paragraph::new("Tab: 次の項目   Ctrl+S: 登録   Esc: キャンセル")
                    .style(Style::default().fg(MUTED)),
                parts[4],
            );
        }
        Modal::Search(editor) => {
            let area = popup(frame.area(), 70, 5);
            frame.render_widget(Clear, area);
            frame.render_widget(panel("設定を検索 · Enterで適用 / Escで閉じる", true), area);
            draw_editor(
                frame,
                editor,
                area.inner(ratatui::layout::Margin::new(2, 2)),
            );
        }
        Modal::Confirm { action, scroll } => {
            let area = popup(
                frame.area(),
                90,
                if matches!(action, Action::Save) {
                    frame.area().height.saturating_sub(4)
                } else {
                    12
                },
            );
            frame.render_widget(Clear, area);
            frame.render_widget(panel("確認", true), area);
            let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(2)])
                .split(area.inner(ratatui::layout::Margin::new(2, 1)));
            let mut lines = Vec::new();
            match action {
                Action::Save => {
                    if let Some(config) = app.config() {
                        lines.push(Line::from(Span::styled(
                            format!("保存先: {}", short_path(&config.snapshot.path, app)),
                            Style::default().fg(ACCENT),
                        )));
                        lines.push(Line::from(
                            "既存ファイルはバックアップしてから置き換えます。",
                        ));
                        lines.push(Line::from(""));
                        for change in
                            TextDiff::from_lines(&config.original, &config.text).iter_all_changes()
                        {
                            let (prefix, color) = match change.tag() {
                                ChangeTag::Delete => ("- ", RED),
                                ChangeTag::Insert => ("+ ", ACCENT),
                                ChangeTag::Equal => ("  ", MUTED),
                            };
                            lines.push(Line::from(Span::styled(
                                format!("{prefix}{}", safe(change.value()).trim_end_matches('\n')),
                                Style::default().fg(color),
                            )));
                        }
                    }
                }
                Action::Reset(path) => {
                    lines.push(Line::from(format!(
                        "{} をファイルから削除し、未設定に戻します。",
                        safe(&path.join("."))
                    )));
                    lines.push(Line::from("変更は保存するまでファイルに書き込みません。"));
                }
                Action::Reload => lines.push(Line::from(
                    "このプロファイルの未保存の変更を破棄し、再読み込みします。",
                )),
                Action::RemoveProfile => {
                    lines.push(Line::from(format!(
                        "{} の登録を解除します。",
                        safe(&app.profiles.items[app.selected].name)
                    )));
                    lines.push(Line::from(
                        "設定ファイルは保持します。このプロファイルの未保存の変更は破棄します。",
                    ));
                }
                Action::Quit => {
                    lines.push(Line::from(format!(
                        "{}件のプロファイルに未保存の変更があります。",
                        app.dirty_count()
                    )));
                    lines.push(Line::from("すべての未保存の変更を破棄して終了します。"));
                    lines.push(Line::from(
                        "保存する場合はEscで戻り、各プロファイルでCtrl+Sを押してください。",
                    ));
                }
            }
            let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
            let max_scroll = paragraph
                .line_count(parts[0].width)
                .saturating_sub(parts[0].height as usize)
                .min(u16::MAX as usize) as u16;
            *scroll = (*scroll).min(max_scroll);
            frame.render_widget(paragraph.scroll((*scroll, 0)), parts[0]);
            frame.render_widget(
                Paragraph::new(
                    "Enter / y: 実行   Esc / n: キャンセル   ↑↓ / PgUp/PgDn: スクロール",
                )
                .style(Style::default().fg(AMBER))
                .wrap(Wrap { trim: false }),
                parts[1],
            );
        }
        Modal::Help => {
            let area = popup(frame.area(), 82, 23);
            frame.render_widget(Clear, area);
            frame.render_widget(panel("キーボード操作", true), area);
            frame.render_widget(Paragraph::new("Tab / ← →     プロファイルと設定のペインを切替\n↑↓ / j k      選択を移動（PgUp / PgDn: 10件移動）\n[ / ]         前 / 次のプロファイルへ移動\nEnter / e     選択した設定を編集\nSpace         真偽値を切替\nE / F4        JSON / TOML全体を編集\nd             設定キーを削除して未設定に戻す\nCtrl+S / s    差分を確認して選択中のファイルを保存\nr             ファイルを再読み込み\n/             キー・説明で検索（Esc: 検索解除）\na             プロファイルを追加（Ctrl+Sで登録）\nx             プロファイルの登録解除（左ペインで操作）\nh             値のマスクを切替\nq / Ctrl+C    終了（未保存の変更があれば確認）\n\n編集画面: Ctrl+Sで反映、Escでキャンセル。\n文字列は引用符不要。配列などはJSON / TOMLで入力。\n全体編集・編集画面・保存差分には秘密情報も表示されます。\n\nEnter / Esc: 閉じる")
                .style(Style::default().fg(TEXT)).wrap(Wrap { trim: false }), area.inner(ratatui::layout::Margin::new(2, 1)));
        }
    }
    app.modal = Some(modal);
}

fn input(frame: &mut Frame, editor: &mut Editor, area: Rect, title: &str, active: bool) {
    frame.render_widget(panel(title, active), area);
    let inner = area.inner(ratatui::layout::Margin::new(1, 1));
    if active {
        draw_editor(frame, editor, inner);
    } else {
        frame.render_widget(Paragraph::new(safe(&editor.text())), inner);
    }
}

fn draw_editor(frame: &mut Frame, editor: &mut Editor, area: Rect) {
    if area.is_empty() {
        return;
    }
    let column = editor.visual_col();
    if editor.row < editor.top {
        editor.top = editor.row;
    }
    if editor.row >= editor.top + area.height as usize {
        editor.top = editor.row + 1 - area.height as usize;
    }
    if column < editor.left {
        editor.left = column;
    }
    if column >= editor.left + area.width as usize {
        editor.left = column + 1 - area.width as usize;
    }
    let text = editor
        .lines
        .iter()
        .skip(editor.top)
        .take(area.height as usize)
        .map(|line| Line::from(safe(&line.replace('\t', "    "))))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(text)
            .scroll((0, editor.left.min(u16::MAX as usize) as u16))
            .style(Style::default().fg(TEXT)),
        area,
    );
    frame.set_cursor_position((
        area.x + (column - editor.left) as u16,
        area.y + (editor.row - editor.top) as u16,
    ));
}

fn popup(area: Rect, percent: u16, height: u16) -> Rect {
    let width = (area.width as u32 * percent as u32 / 100).min(area.width as u32) as u16;
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

fn short_path(path: &std::path::Path, app: &App) -> String {
    safe(
        &path
            .strip_prefix(&app.home)
            .map(|rest| format!("~/{}", rest.display()))
            .unwrap_or_else(|_| path.display().to_string()),
    )
}

fn secret(key: &str) -> bool {
    let key = key.to_lowercase();
    key == "env"
        || [
            "token",
            "secret",
            "password",
            "api_key",
            "apikey",
            "authorization",
        ]
        .iter()
        .any(|word| key.contains(word))
}

fn safe(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() && character != '\n' {
                '�'
            } else {
                character
            }
        })
        .collect()
}
