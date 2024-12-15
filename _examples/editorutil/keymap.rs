use promkit::{
    crossterm::{
        self,
        event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers},
    },
    text_editor,
};

use promkit_async::Event;

pub type Handler = fn(&[Event], &mut text_editor::State) -> anyhow::Result<()>;

pub fn movement(event_buffer: &[Event], state: &mut text_editor::State) -> anyhow::Result<()> {
    for event in event_buffer {
        match event {
            Event::HorizontalCursorBuffer(left, right) => {
                state.texteditor.shift(*left, *right);
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn default(event_buffer: &[Event], state: &mut text_editor::State) -> anyhow::Result<()> {
    for event in event_buffer {
        match event {
            Event::KeyBuffer(chars) => match state.edit_mode {
                text_editor::Mode::Insert => state.texteditor.insert_chars(&chars),
                text_editor::Mode::Overwrite => state.texteditor.overwrite_chars(&chars),
            },
            Event::HorizontalCursorBuffer(left, right) => {
                state.texteditor.shift(*left, *right);
            }
            Event::Others(e, times) => match e {
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('a'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }) => state.texteditor.move_to_head(),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('e'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }) => state.texteditor.move_to_tail(),

                // Erase char(s).
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Backspace,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }) => {
                    for _ in 0..*times {
                        state.texteditor.erase();
                    }
                }
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('u'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }) => state.texteditor.erase_all(),
                _ => {}
            },
            _ => {}
        }
    }

    Ok(())
}
