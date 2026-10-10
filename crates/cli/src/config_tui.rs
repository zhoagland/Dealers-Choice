//! `dc config`: a terminal UI to list, create, copy, edit, and delete saved profiles (FR-UI-3).
//!
//! Design: [`App`] is a plain state machine. `handle(key)` changes state, `render(frame)` draws it.
//! Only `run` touches the real terminal, so all the behavior is unit-testable by feeding key
//! codes. Editing is generic: a profile is shown as its TOML keys and values, each value is
//! edited as TOML text (`"holdem_nl"`, `[7, 5, 3]`, `0.25`), and **nothing is saved unless the
//! whole profile still loads as its real type** (`RunSpec` or `PlayProfile`), so a saved profile
//! always works with `--profile`.

use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};

use games::registry::GAME_IDS;
use sim::RunSpec;
use storage::{ProfileKind, ProfileStore};

use crate::CliResult;
use crate::settings::PlayProfile;
use crate::simulate::default_spec;

/// Starting contents for a new profile of `kind`: the defaults, as TOML.
fn default_table(kind: ProfileKind) -> toml::Table {
    let value = match kind {
        ProfileKind::Sim => toml::Value::try_from(default_spec()),
        ProfileKind::Play => toml::Value::try_from(PlayProfile::default()),
    };
    match value.expect("defaults serialize") {
        toml::Value::Table(t) => t,
        _ => unreachable!(),
    }
}

/// Check that `table` loads as the real profile type and refers to games that exist.
fn validate(kind: ProfileKind, table: &toml::Table) -> Result<(), String> {
    let v = toml::Value::Table(table.clone());
    let check_game = |g: &str| {
        if GAME_IDS.contains(&g) {
            Ok(())
        } else {
            Err(format!(
                "unknown game {g:?} (known: {})",
                GAME_IDS.join(", ")
            ))
        }
    };
    match kind {
        ProfileKind::Sim => {
            let spec: RunSpec = v.try_into().map_err(|e| e.to_string())?;
            check_game(&spec.game)?;
            if spec.bots.len() < 2 {
                return Err("bots: need at least two".into());
            }
            if !(0.0..=1.0).contains(&spec.history_sample) {
                return Err("history_sample must be between 0 and 1".into());
            }
        }
        ProfileKind::Play => {
            let p: PlayProfile = v.try_into().map_err(|e| e.to_string())?;
            if !p.game.is_empty() {
                check_game(&p.game)?;
            }
            for g in &p.pool {
                check_game(g)?;
            }
            if p.bots.is_empty() || p.bots.iter().any(|&l| !(1..=10).contains(&l)) {
                return Err("bots: one level 1-10 per bot seat".into());
            }
        }
    }
    Ok(())
}

/// One profile open for editing.
struct Editor {
    name: String,
    /// (key, value as TOML text), in file order.
    fields: Vec<(String, String)>,
    selected: usize,
    /// `Some(text)` while a value is being typed.
    input: Option<String>,
    dirty: bool,
    /// Esc was pressed once with unsaved changes.
    confirm_discard: bool,
}

impl Editor {
    fn from_table(name: &str, table: &toml::Table) -> Editor {
        Editor {
            name: name.to_string(),
            fields: table
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
            selected: 0,
            input: None,
            dirty: false,
            confirm_discard: false,
        }
    }

    /// Rebuild a TOML table from the field texts.
    fn to_table(&self) -> Result<toml::Table, String> {
        let mut t = toml::Table::new();
        for (k, text) in &self.fields {
            t.insert(
                k.clone(),
                parse_value(text).map_err(|e| format!("{k}: {e}"))?,
            );
        }
        Ok(t)
    }
}

/// Parse one TOML value from text, e.g. `[7, 5, 3]`.
fn parse_value(text: &str) -> Result<toml::Value, String> {
    let doc: toml::Table = format!("v = {text}")
        .parse()
        .map_err(|_| format!("not a TOML value: {text}"))?;
    Ok(doc["v"].clone())
}

enum Prompt {
    NewName(String),
    CopyName(String),
    ConfirmDelete,
    /// `key = value` to add a field the defaults leave out (e.g. `stack = 5000`).
    AddField(String),
}

pub struct App {
    store: ProfileStore,
    kind: ProfileKind,
    names: Vec<String>,
    selected: usize,
    editor: Option<Editor>,
    prompt: Option<Prompt>,
    pub message: String,
    pub quit: bool,
}

impl App {
    pub fn new(store: ProfileStore) -> App {
        let mut app = App {
            store,
            kind: ProfileKind::Sim,
            names: Vec::new(),
            selected: 0,
            editor: None,
            prompt: None,
            message: String::new(),
            quit: false,
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        self.names = self.store.list(self.kind);
        self.selected = self.selected.min(self.names.len().saturating_sub(1));
    }

    fn open(&mut self, name: &str) {
        match self.store.load_table(self.kind, name) {
            Ok(t) => self.editor = Some(Editor::from_table(name, &t)),
            Err(e) => self.message = e,
        }
    }

    pub fn handle(&mut self, key: KeyCode) {
        if self.prompt.is_some() {
            self.handle_prompt(key);
        } else if self.editor.is_some() {
            self.handle_editor(key);
        } else {
            self.handle_list(key);
        }
    }

    fn handle_list(&mut self, key: KeyCode) {
        self.message.clear();
        match key {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Tab | KeyCode::Left | KeyCode::Right => {
                self.kind = if self.kind == ProfileKind::Sim {
                    ProfileKind::Play
                } else {
                    ProfileKind::Sim
                };
                self.selected = 0;
                self.refresh();
            }
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.names.len().saturating_sub(1))
            }
            KeyCode::Enter => {
                if let Some(name) = self.names.get(self.selected).cloned() {
                    self.open(&name);
                }
            }
            KeyCode::Char('n') => self.prompt = Some(Prompt::NewName(String::new())),
            KeyCode::Char('c') if !self.names.is_empty() => {
                self.prompt = Some(Prompt::CopyName(String::new()))
            }
            KeyCode::Char('d') if !self.names.is_empty() => {
                self.prompt = Some(Prompt::ConfirmDelete)
            }
            _ => {}
        }
    }

    fn handle_prompt(&mut self, key: KeyCode) {
        let prompt = self.prompt.take().unwrap();
        match (prompt, key) {
            (_, KeyCode::Esc) => {}
            (Prompt::ConfirmDelete, KeyCode::Char('y')) => {
                let name = self.names[self.selected].clone();
                match self.store.delete(self.kind, &name) {
                    Ok(()) => self.message = format!("deleted {name}"),
                    Err(e) => self.message = e,
                }
                self.refresh();
            }
            (Prompt::ConfirmDelete, _) => {}
            (Prompt::NewName(s) | Prompt::CopyName(s), KeyCode::Enter)
                if !ProfileStore::valid_name(&s) =>
            {
                self.message = "names use letters, digits, - and _".into();
            }
            (Prompt::NewName(s), KeyCode::Enter) => self.create(&s, default_table(self.kind)),
            (Prompt::CopyName(s), KeyCode::Enter) => {
                let from = self.names[self.selected].clone();
                match self.store.load_table(self.kind, &from) {
                    Ok(t) => self.create(&s, t),
                    Err(e) => self.message = e,
                }
            }
            (Prompt::AddField(s), KeyCode::Enter) => {
                let parsed = s
                    .split_once('=')
                    .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()));
                match parsed {
                    Some((k, v)) if !k.is_empty() && parse_value(&v).is_ok() => {
                        let ed = self.editor.as_mut().unwrap();
                        ed.fields.push((k, v));
                        ed.selected = ed.fields.len() - 1;
                        ed.dirty = true;
                    }
                    _ => self.message = "expected key = value, e.g. stack = 5000".into(),
                }
            }
            (Prompt::NewName(mut s), k) => {
                edit_text(&mut s, k);
                self.prompt = Some(Prompt::NewName(s));
            }
            (Prompt::CopyName(mut s), k) => {
                edit_text(&mut s, k);
                self.prompt = Some(Prompt::CopyName(s));
            }
            (Prompt::AddField(mut s), k) => {
                edit_text(&mut s, k);
                self.prompt = Some(Prompt::AddField(s));
            }
        }
    }

    /// Save a new profile and open it.
    fn create(&mut self, name: &str, table: toml::Table) {
        if self.store.list(self.kind).iter().any(|n| n == name) {
            self.message = format!("{name} already exists");
            return;
        }
        match self.store.save(self.kind, name, &table) {
            Ok(_) => {
                self.refresh();
                self.selected = self.names.iter().position(|n| n == name).unwrap_or(0);
                self.open(name);
            }
            Err(e) => self.message = e,
        }
    }

    fn handle_editor(&mut self, key: KeyCode) {
        let kind = self.kind;
        let ed = self.editor.as_mut().unwrap();
        if let Some(input) = ed.input.as_mut() {
            match key {
                KeyCode::Esc => ed.input = None,
                KeyCode::Enter => match parse_value(input) {
                    Ok(_) => {
                        ed.fields[ed.selected].1 = input.trim().to_string();
                        ed.input = None;
                        ed.dirty = true;
                        self.message.clear();
                    }
                    Err(e) => self.message = e,
                },
                k => edit_text(input, k),
            }
            return;
        }
        match key {
            KeyCode::Up => ed.selected = ed.selected.saturating_sub(1),
            KeyCode::Down => ed.selected = (ed.selected + 1).min(ed.fields.len().saturating_sub(1)),
            KeyCode::Enter if !ed.fields.is_empty() => {
                ed.input = Some(ed.fields[ed.selected].1.clone());
                ed.confirm_discard = false;
            }
            KeyCode::Char('a') => self.prompt = Some(Prompt::AddField(String::new())),
            KeyCode::Char('s') => {
                let result = ed.to_table().and_then(|t| validate(kind, &t).map(|_| t));
                match result {
                    Ok(t) => match self.store.save(kind, &ed.name, &t) {
                        Ok(path) => {
                            ed.dirty = false;
                            self.message = format!("saved {}", path.display());
                        }
                        Err(e) => self.message = e,
                    },
                    Err(e) => self.message = format!("not saved: {e}"),
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                if ed.dirty && !ed.confirm_discard {
                    ed.confirm_discard = true;
                    self.message = "unsaved changes: s to save, Esc again to discard".into();
                } else {
                    self.editor = None;
                    self.message.clear();
                }
            }
            _ => {}
        }
    }

    pub fn render(&self, f: &mut Frame) {
        let [title, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .areas(f.area());
        let kind_label = |k: ProfileKind| {
            let label = match k {
                ProfileKind::Sim => " sim profiles ",
                ProfileKind::Play => " play profiles ",
            };
            if k == self.kind {
                label.reversed()
            } else {
                label.into()
            }
        };
        f.render_widget(
            Line::from(vec![
                " dc config ".bold(),
                kind_label(ProfileKind::Sim),
                kind_label(ProfileKind::Play),
            ]),
            title,
        );

        // Body: the open profile's fields, or the list of profiles.
        let selected_style = Style::new().add_modifier(Modifier::REVERSED);
        match &self.editor {
            Some(ed) => {
                let items: Vec<ListItem> = ed
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(i, (k, v))| {
                        let shown = match (&ed.input, i == ed.selected) {
                            (Some(input), true) => format!("{input}_"),
                            _ => v.clone(),
                        };
                        ListItem::new(format!("{k:<18} = {shown}"))
                    })
                    .collect();
                let title = format!(
                    " {}/{}{} ",
                    self.kind.dir_name(),
                    ed.name,
                    if ed.dirty { " *" } else { "" }
                );
                let mut state = ListState::default().with_selected(Some(ed.selected));
                f.render_stateful_widget(
                    List::new(items)
                        .block(Block::bordered().title(title))
                        .highlight_style(selected_style),
                    body,
                    &mut state,
                );
            }
            None => {
                let items: Vec<ListItem> = if self.names.is_empty() {
                    vec![ListItem::new("(none yet: press n to create one)")]
                } else {
                    self.names
                        .iter()
                        .map(|n| ListItem::new(n.as_str()))
                        .collect()
                };
                let title = format!(" config/{}/ ", self.kind.dir_name());
                let mut state = ListState::default()
                    .with_selected((!self.names.is_empty()).then_some(self.selected));
                f.render_stateful_widget(
                    List::new(items)
                        .block(Block::bordered().title(title))
                        .highlight_style(selected_style),
                    body,
                    &mut state,
                );
            }
        }

        // Footer: the active prompt, or the keys that work right now.
        let help = match (&self.prompt, &self.editor) {
            (Some(Prompt::NewName(s)), _) => {
                format!("new profile name: {s}_   (Enter create, Esc cancel)")
            }
            (Some(Prompt::CopyName(s)), _) => format!("copy as: {s}_   (Enter copy, Esc cancel)"),
            (Some(Prompt::ConfirmDelete), _) => {
                "delete this profile? y / any other key cancels".into()
            }
            (Some(Prompt::AddField(s)), _) => format!("add field (key = value): {s}_"),
            (None, Some(ed)) if ed.input.is_some() => {
                "type a TOML value · Enter keep · Esc cancel".into()
            }
            (None, Some(_)) => "↑↓ select · Enter edit · a add field · s save · Esc back".into(),
            (None, None) => {
                "Tab switch kind · ↑↓ select · Enter edit · n new · c copy · d delete · q quit"
                    .into()
            }
        };
        f.render_widget(
            Paragraph::new(vec![
                Line::from(help),
                Line::from(self.message.as_str().yellow()),
            ])
            .block(Block::bordered()),
            footer,
        );
    }
}

/// Typing into a one-line text field.
fn edit_text(s: &mut String, key: KeyCode) {
    match key {
        KeyCode::Char(c) => s.push(c),
        KeyCode::Backspace => {
            s.pop();
        }
        _ => {}
    }
}

/// Run the TUI until the user quits. `ratatui::init` switches to the alternate screen and raw
/// mode; `restore` always undoes that, even if drawing failed.
pub fn run(store: ProfileStore) -> CliResult {
    let mut terminal = ratatui::init();
    let mut app = App::new(store);
    let result = (|| -> std::io::Result<()> {
        while !app.quit {
            terminal.draw(|f| app.render(f))?;
            // Windows reports key presses *and* releases; act on presses only.
            if let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                app.handle(k.code);
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(tag: &str) -> (ProfileStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("dc-tui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (ProfileStore::resolve(Some(&dir)), dir)
    }

    fn type_text(app: &mut App, s: &str) {
        for c in s.chars() {
            app.handle(KeyCode::Char(c));
        }
    }

    /// Select the field named `key` in the open editor.
    fn select(app: &mut App, key: &str) {
        let ed = app.editor.as_mut().unwrap();
        ed.selected = ed.fields.iter().position(|(k, _)| k == key).unwrap();
    }

    fn replace_value(app: &mut App, key: &str, value: &str) {
        select(app, key);
        app.handle(KeyCode::Enter);
        for _ in 0..200 {
            app.handle(KeyCode::Backspace);
        }
        type_text(app, value);
        app.handle(KeyCode::Enter);
    }

    #[test]
    fn create_edit_save_sim_profile() {
        let (store, dir) = temp_store("sim");
        let mut app = App::new(store.clone());
        app.handle(KeyCode::Char('n'));
        type_text(&mut app, "nightly");
        app.handle(KeyCode::Enter);
        assert!(
            app.editor.is_some(),
            "new profile opens in the editor: {}",
            app.message
        );
        replace_value(&mut app, "deals", "50");
        app.handle(KeyCode::Char('s'));
        assert!(app.message.starts_with("saved"), "{}", app.message);
        let spec: RunSpec = store.load(ProfileKind::Sim, "nightly").unwrap();
        assert_eq!(spec.deals, 50);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_profile_is_not_saved() {
        let (store, dir) = temp_store("bad");
        let mut app = App::new(store.clone());
        app.handle(KeyCode::Char('n'));
        type_text(&mut app, "p");
        app.handle(KeyCode::Enter);
        replace_value(&mut app, "game", "\"nope\"");
        app.handle(KeyCode::Char('s'));
        assert!(app.message.starts_with("not saved"), "{}", app.message);
        let spec: RunSpec = store.load(ProfileKind::Sim, "p").unwrap();
        assert_eq!(spec.game, "holdem_nl", "file unchanged");
        // Esc with unsaved changes asks first; a second Esc discards.
        app.handle(KeyCode::Esc);
        assert!(app.editor.is_some());
        app.handle(KeyCode::Esc);
        assert!(app.editor.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn play_profiles_copy_and_delete() {
        let (store, dir) = temp_store("play");
        let mut app = App::new(store.clone());
        app.handle(KeyCode::Tab); // switch to play profiles
        app.handle(KeyCode::Char('n'));
        type_text(&mut app, "evening");
        app.handle(KeyCode::Enter);
        replace_value(&mut app, "bots", "[9, 9]");
        app.handle(KeyCode::Char('s'));
        app.handle(KeyCode::Esc);
        app.handle(KeyCode::Char('c'));
        type_text(&mut app, "evening2");
        app.handle(KeyCode::Enter);
        app.handle(KeyCode::Esc);
        assert_eq!(store.list(ProfileKind::Play), vec!["evening", "evening2"]);
        let copied: PlayProfile = store.load(ProfileKind::Play, "evening2").unwrap();
        assert_eq!(copied.bots, vec![9, 9]);
        app.handle(KeyCode::Char('d'));
        app.handle(KeyCode::Char('y'));
        assert_eq!(store.list(ProfileKind::Play).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bad_toml_value_is_rejected_while_typing() {
        let (store, dir) = temp_store("val");
        let mut app = App::new(store);
        app.handle(KeyCode::Char('n'));
        type_text(&mut app, "x");
        app.handle(KeyCode::Enter);
        select(&mut app, "deals");
        app.handle(KeyCode::Enter);
        type_text(&mut app, " ]]");
        app.handle(KeyCode::Enter);
        assert!(
            app.message.starts_with("not a TOML value"),
            "{}",
            app.message
        );
        assert!(
            app.editor.as_ref().unwrap().input.is_some(),
            "still editing"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
