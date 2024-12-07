use std::time::Duration;

use futures::future::Future;
use futures_timer::Delay;
use promkit::crossterm::{
    self,
    event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers},
};
use tokio::sync::mpsc;

use crate::event::Event;

pub struct TimeBasedOperator {}

impl TimeBasedOperator {
    pub fn run(
        &mut self,
        delay: Duration,
        mut receiver: mpsc::Receiver<crossterm::event::Event>,
        sender: mpsc::Sender<Vec<Event>>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        let mut buffer = Vec::new();

        async move {
            loop {
                let delay = Delay::new(delay);
                futures::pin_mut!(delay);

                tokio::select! {
                    maybe_event = receiver.recv() => {
                        if let Some(event) = maybe_event {
                            buffer.push(event);
                        } else {
                            break;
                        }
                    },
                    _ = delay => {
                        if !buffer.is_empty() {
                            let bundles = Self::process_events(&buffer);
                            if !bundles.is_empty() {
                                sender.send(bundles).await?;
                            }
                            buffer.clear();
                        }
                    },
                }
            }
            Ok(())
        }
    }

    fn process_events(events: &Vec<crossterm::event::Event>) -> Vec<Event> {
        let mut result = Vec::new();
        let mut current_chars = Vec::new();
        let mut current_vertical = (0, 0);
        let mut current_horizontal = (0, 0);
        let mut current_others: Option<(crossterm::event::Event, usize)> = None;
        let mut last_resize: Option<(u16, u16)> = None;
        let mut resize_index: Option<usize> = None;

        for event in events {
            match event {
                crossterm::event::Event::Resize(width, height) => {
                    Self::flush_all_buffers(
                        &mut result,
                        &mut current_chars,
                        &mut current_vertical,
                        &mut current_horizontal,
                        &mut current_others,
                    );
                    last_resize = Some((*width, *height));
                    resize_index = Some(result.len());
                }
                event if Self::extract_char(&event).is_some() => {
                    let ch = Self::extract_char(&event).unwrap();
                    Self::flush_non_char_buffers(
                        &mut result,
                        &mut current_vertical,
                        &mut current_horizontal,
                        &mut current_others,
                    );
                    current_chars.push(ch);
                }
                event if Self::detect_vertical_direction(&event).is_some() => {
                    let (up, down) = Self::detect_vertical_direction(&event).unwrap();
                    Self::flush_char_buffer(&mut result, &mut current_chars);
                    Self::flush_horizontal_buffer(&mut result, &mut current_horizontal);
                    Self::flush_others_buffer(&mut result, &mut current_others);
                    current_vertical.0 += up;
                    current_vertical.1 += down;
                }
                event if Self::detect_horizontal_direction(&event).is_some() => {
                    let (left, right) = Self::detect_horizontal_direction(&event).unwrap();
                    Self::flush_char_buffer(&mut result, &mut current_chars);
                    Self::flush_vertical_buffer(&mut result, &mut current_vertical);
                    Self::flush_others_buffer(&mut result, &mut current_others);
                    current_horizontal.0 += left;
                    current_horizontal.1 += right;
                }
                _ => {
                    Self::flush_char_buffer(&mut result, &mut current_chars);
                    Self::flush_vertical_buffer(&mut result, &mut current_vertical);
                    Self::flush_horizontal_buffer(&mut result, &mut current_horizontal);

                    match &mut current_others {
                        Some((last_event, count)) if last_event == event => {
                            *count += 1;
                        }
                        _ => {
                            Self::flush_others_buffer(&mut result, &mut current_others);
                            current_others = Some((event.clone(), 1));
                        }
                    }
                }
            }
        }

        // Flush remaining buffers
        Self::flush_all_buffers(
            &mut result,
            &mut current_chars,
            &mut current_vertical,
            &mut current_horizontal,
            &mut current_others,
        );

        // Add the last resize event if exists at the recorded index
        if let (Some((width, height)), Some(idx)) = (last_resize, resize_index) {
            result.insert(idx, Event::LastResize(width, height));
        }

        result
    }

    fn flush_all_buffers(
        result: &mut Vec<Event>,
        chars: &mut Vec<char>,
        vertical: &mut (usize, usize),
        horizontal: &mut (usize, usize),
        others: &mut Option<(crossterm::event::Event, usize)>,
    ) {
        Self::flush_char_buffer(result, chars);
        Self::flush_vertical_buffer(result, vertical);
        Self::flush_horizontal_buffer(result, horizontal);
        Self::flush_others_buffer(result, others);
    }

    fn flush_char_buffer(result: &mut Vec<Event>, chars: &mut Vec<char>) {
        if !chars.is_empty() {
            result.push(Event::KeyBuffer(chars.clone()));
            chars.clear();
        }
    }

    fn flush_vertical_buffer(result: &mut Vec<Event>, vertical: &mut (usize, usize)) {
        if *vertical != (0, 0) {
            result.push(Event::VerticalCursorBuffer(vertical.0, vertical.1));
            *vertical = (0, 0);
        }
    }

    fn flush_horizontal_buffer(result: &mut Vec<Event>, horizontal: &mut (usize, usize)) {
        if *horizontal != (0, 0) {
            result.push(Event::HorizontalCursorBuffer(horizontal.0, horizontal.1));
            *horizontal = (0, 0);
        }
    }

    fn flush_others_buffer(
        result: &mut Vec<Event>,
        others: &mut Option<(crossterm::event::Event, usize)>,
    ) {
        if let Some((event, count)) = others.take() {
            result.push(Event::Others(event, count));
        }
    }

    fn flush_non_char_buffers(
        result: &mut Vec<Event>,
        vertical: &mut (usize, usize),
        horizontal: &mut (usize, usize),
        others: &mut Option<(crossterm::event::Event, usize)>,
    ) {
        Self::flush_vertical_buffer(result, vertical);
        Self::flush_horizontal_buffer(result, horizontal);
        Self::flush_others_buffer(result, others);
    }

    fn extract_char(event: &crossterm::event::Event) -> Option<char> {
        match event {
            crossterm::event::Event::Key(KeyEvent {
                code: KeyCode::Char(ch),
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            })
            | crossterm::event::Event::Key(KeyEvent {
                code: KeyCode::Char(ch),
                modifiers: KeyModifiers::SHIFT,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            }) => Some(*ch),
            _ => None,
        }
    }

    fn detect_vertical_direction(event: &crossterm::event::Event) -> Option<(usize, usize)> {
        match event {
            crossterm::event::Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => Some((1, 0)),
            crossterm::event::Event::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            }) => Some((0, 1)),
            _ => None,
        }
    }

    fn detect_horizontal_direction(event: &crossterm::event::Event) -> Option<(usize, usize)> {
        match event {
            crossterm::event::Event::Key(KeyEvent {
                code: KeyCode::Left,
                ..
            }) => Some((1, 0)),
            crossterm::event::Event::Key(KeyEvent {
                code: KeyCode::Right,
                ..
            }) => Some((0, 1)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod process_events {
        use super::*;

        #[test]
        fn test() {
            let events = vec![
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('a'),
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('B'),
                    modifiers: KeyModifiers::SHIFT,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Resize(128, 128),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Up,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Down,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Up,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Left,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Right,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Left,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('f'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('f'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('f'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('d'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Up,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                crossterm::event::Event::Resize(64, 64),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('d'),
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
            ];

            let expected = vec![
                Event::KeyBuffer(vec!['a', 'B', 'c']),
                Event::VerticalCursorBuffer(2, 1),
                Event::HorizontalCursorBuffer(2, 1),
                Event::Others(
                    crossterm::event::Event::Key(KeyEvent {
                        code: KeyCode::Char('f'),
                        modifiers: KeyModifiers::CONTROL,
                        kind: KeyEventKind::Press,
                        state: KeyEventState::NONE,
                    }),
                    3,
                ),
                Event::Others(
                    crossterm::event::Event::Key(KeyEvent {
                        code: KeyCode::Char('d'),
                        modifiers: KeyModifiers::CONTROL,
                        kind: KeyEventKind::Press,
                        state: KeyEventState::NONE,
                    }),
                    1,
                ),
                Event::VerticalCursorBuffer(1, 0),
                Event::LastResize(64, 64),
                Event::KeyBuffer(vec!['d']),
            ];

            assert_eq!(TimeBasedOperator::process_events(&events), expected);
        }

        #[test]
        fn test_only_others() {
            let events = vec![crossterm::event::Event::Key(KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            })];

            let expected = vec![Event::Others(
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                1,
            )];

            assert_eq!(TimeBasedOperator::process_events(&events), expected);
        }
    }
}
