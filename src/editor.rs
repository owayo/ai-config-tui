use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug)]
pub struct Editor {
    pub lines: Vec<String>,
    pub row: usize,
    pub col: usize,
    pub top: usize,
    pub left: usize,
}

impl Editor {
    pub fn new(text: &str) -> Self {
        Self {
            lines: text.split('\n').map(String::from).collect(),
            row: 0,
            col: if text.contains('\n') {
                0
            } else {
                text.chars().count()
            },
            top: 0,
            left: 0,
        }
    }
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }
    pub fn insert(&mut self, text: &str, multiline: bool) {
        for character in text.replace("\r\n", "\n").chars() {
            if character == '\n' && multiline {
                let offset = self.byte_offset();
                let rest = self.lines[self.row].split_off(offset);
                self.row += 1;
                self.col = 0;
                self.lines.insert(self.row, rest);
            } else if character == '\t' {
                self.insert("  ", multiline);
            } else if !character.is_control() {
                let offset = self.byte_offset();
                self.lines[self.row].insert(offset, character);
                self.col += 1;
            }
        }
    }

    pub fn handle(&mut self, key: KeyEvent, multiline: bool) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('a') => self.col = 0,
                KeyCode::Char('e') => self.col = self.lines[self.row].chars().count(),
                KeyCode::Char('u') => {
                    let offset = self.byte_offset();
                    self.lines[self.row].drain(..offset);
                    self.col = 0;
                }
                KeyCode::Char('k') => {
                    let offset = self.byte_offset();
                    self.lines[self.row].truncate(offset);
                }
                KeyCode::Home => {
                    self.row = 0;
                    self.col = 0;
                }
                KeyCode::End => {
                    self.row = self.lines.len() - 1;
                    self.col = self.lines[self.row].chars().count();
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::ALT) => {
                self.insert(&character.to_string(), multiline)
            }
            KeyCode::Enter if multiline => self.insert("\n", true),
            KeyCode::Tab => self.insert("  ", multiline),
            KeyCode::Left if self.col > 0 => self.col -= 1,
            KeyCode::Left if self.row > 0 => {
                self.row -= 1;
                self.col = self.lines[self.row].chars().count();
            }
            KeyCode::Right if self.col < self.lines[self.row].chars().count() => self.col += 1,
            KeyCode::Right if self.row + 1 < self.lines.len() => {
                self.row += 1;
                self.col = 0;
            }
            KeyCode::Up => {
                self.row = self.row.saturating_sub(1);
                self.clamp_column();
            }
            KeyCode::Down => {
                self.row = (self.row + 1).min(self.lines.len() - 1);
                self.clamp_column();
            }
            KeyCode::PageUp => {
                self.row = self.row.saturating_sub(10);
                self.clamp_column();
            }
            KeyCode::PageDown => {
                self.row = (self.row + 10).min(self.lines.len() - 1);
                self.clamp_column();
            }
            KeyCode::Home => self.col = 0,
            KeyCode::End => self.col = self.lines[self.row].chars().count(),
            KeyCode::Backspace if self.col > 0 => {
                self.col -= 1;
                let offset = self.byte_offset();
                self.lines[self.row].remove(offset);
            }
            KeyCode::Backspace if self.row > 0 => {
                let line = self.lines.remove(self.row);
                self.row -= 1;
                self.col = self.lines[self.row].chars().count();
                self.lines[self.row].push_str(&line);
            }
            KeyCode::Delete if self.col < self.lines[self.row].chars().count() => {
                let offset = self.byte_offset();
                self.lines[self.row].remove(offset);
            }
            KeyCode::Delete if self.row + 1 < self.lines.len() => {
                let line = self.lines.remove(self.row + 1);
                self.lines[self.row].push_str(&line);
            }
            _ => {}
        }
    }

    pub fn visual_col(&self) -> usize {
        self.lines[self.row][..self.byte_offset()]
            .replace('\t', "    ")
            .width()
    }
    fn byte_offset(&self) -> usize {
        self.lines[self.row]
            .char_indices()
            .nth(self.col)
            .map_or(self.lines[self.row].len(), |(offset, _)| offset)
    }
    fn clamp_column(&mut self) {
        self.col = self.col.min(self.lines[self.row].chars().count());
    }
}
