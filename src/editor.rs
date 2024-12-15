use crossterm::{
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers},
    style::Color,
};
use promkit::{
    pane::Pane, style::StyleBuilder, switch::ActiveKeySwitcher, text_editor, PaneFactory,
};

use crate::Action;

pub struct Editor {
    keymap: ActiveKeySwitcher<Keybinding>,
    state: text_editor::State,
}

impl Editor {
    pub fn text(&self) -> String {
        self.state.texteditor.text_without_cursor().to_string()
    }

    pub fn create_pane(&self, width: u16, height: u16) -> Pane {
        self.state.create_pane(width, height)
    }

    pub fn evaluate(&mut self, event: &Event) -> anyhow::Result<Action> {
        let keymap = self.keymap.get();
        keymap(event, &mut self.state)
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            keymap: ActiveKeySwitcher::new("default", default),
            state: text_editor::State {
                texteditor: Default::default(),
                history: Default::default(),
                prefix: String::from("❯❯ "),
                mask: Default::default(),
                prefix_style: StyleBuilder::new().fgc(Color::DarkGreen).build(),
                active_char_style: StyleBuilder::new().bgc(Color::DarkCyan).build(),
                inactive_char_style: StyleBuilder::new().build(),
                edit_mode: Default::default(),
                word_break_chars: Default::default(),
                lines: Default::default(),
            },
        }
    }
}

pub type Keybinding = fn(&Event, &mut text_editor::State) -> anyhow::Result<Action>;
pub fn default(event: &Event, editor_state: &mut text_editor::State) -> anyhow::Result<Action> {
    let mut action = Action::None;

    match event {
        Event::Key(KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }) => {
            action = Action::Quit;
        }

        // Move cursor.
        Event::Key(KeyEvent {
            code: KeyCode::Left,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }) => {
            editor_state.texteditor.backward();
            action = Action::MoveCursor;
        }
        Event::Key(KeyEvent {
            code: KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }) => {
            editor_state.texteditor.forward();
            action = Action::MoveCursor;
        }
        Event::Key(KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }) => {
            editor_state.texteditor.move_to_head();
            action = Action::MoveCursor;
        }
        Event::Key(KeyEvent {
            code: KeyCode::Char('e'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }) => {
            editor_state.texteditor.move_to_tail();
            action = Action::MoveCursor;
        }

        // Move cursor to the nearest character.
        Event::Key(KeyEvent {
            code: KeyCode::Char('b'),
            modifiers: KeyModifiers::ALT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }) => {
            editor_state
                .texteditor
                .move_to_previous_nearest(&editor_state.word_break_chars);
            action = Action::MoveCursor;
        }

        Event::Key(KeyEvent {
            code: KeyCode::Char('f'),
            modifiers: KeyModifiers::ALT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }) => {
            editor_state
                .texteditor
                .move_to_next_nearest(&editor_state.word_break_chars);
            action = Action::MoveCursor;
        }

        // Erase char(s).
        Event::Key(KeyEvent {
            code: KeyCode::Backspace,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }) => {
            editor_state.texteditor.erase();
            action = Action::ChangeText;
        }
        Event::Key(KeyEvent {
            code: KeyCode::Char('u'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }) => {
            editor_state.texteditor.erase_all();
            action = Action::ChangeText;
        }

        // Erase to the nearest character.
        Event::Key(KeyEvent {
            code: KeyCode::Char('w'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }) => {
            editor_state
                .texteditor
                .erase_to_previous_nearest(&editor_state.word_break_chars);
            action = Action::ChangeText;
        }

        Event::Key(KeyEvent {
            code: KeyCode::Char('d'),
            modifiers: KeyModifiers::ALT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }) => {
            editor_state
                .texteditor
                .erase_to_next_nearest(&editor_state.word_break_chars);
            action = Action::ChangeText;
        }

        // Input char.
        Event::Key(KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
        | Event::Key(KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }) => {
            match editor_state.edit_mode {
                text_editor::Mode::Insert => editor_state.texteditor.insert(*ch),
                text_editor::Mode::Overwrite => editor_state.texteditor.overwrite(*ch),
            }
            action = Action::ChangeText;
        }

        _ => {
            action = Action::None;
        }
    }
    Ok(action)
}
