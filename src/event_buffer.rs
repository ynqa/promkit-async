use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use futures::future::Future;
use futures_timer::Delay;
use tokio::sync::mpsc::{Receiver, Sender};

#[derive(Clone, Debug, PartialEq)]
pub enum EventBundle {
    KeyBuffer(Vec<char>),
    VerticalCursorBuffer(usize, usize),   // (up, down)
    HorizontalCursorBuffer(usize, usize), // (left, right)
    Resize(u16, u16),                     // (width, height)
    Others(Event, usize),
}

pub struct EventBuffer {
    delay_duration: Duration,
}

impl EventBuffer {
    pub fn new(delay_duration: Duration) -> Self {
        EventBuffer { delay_duration }
    }

    pub fn run(
        &mut self,
        mut event_receiver: Receiver<Event>,
        event_buffer_sender: Sender<Vec<EventBundle>>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        let mut buffer = Vec::new();
        let delay_duration = self.delay_duration;

        async move {
            loop {
                let delay = Delay::new(delay_duration);
                futures::pin_mut!(delay);

                tokio::select! {
                    maybe_event = event_receiver.recv() => {
                        if let Some(event) = maybe_event {
                            buffer.push(event);
                        } else {
                            break;
                        }
                    },
                    _ = delay => {
                        if !buffer.is_empty() {
                            event_buffer_sender.send(Self::process_events(buffer.clone())).await?;
                            buffer.clear();
                        }
                    },
                }
            }
            Ok(())
        }
    }

    pub fn run_resize(
        &self,
        mut resize_receiver: Receiver<(u16, u16)>,
        resize_sender: Sender<EventBundle>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        let delay_duration = self.delay_duration;

        async move {
            let mut last_event: Option<(u16, u16)> = None;
            loop {
                let delay = Delay::new(delay_duration);
                futures::pin_mut!(delay);

                tokio::select! {
                    resize_opt = resize_receiver.recv() => {
                        if let Some(event) = resize_opt {
                            last_event = Some(event);
                        } else {
                            break;
                        }
                    },
                    _ = delay => {
                        if let Some((width, height)) = last_event.take() {
                            resize_sender.send(EventBundle::Resize(width, height)).await?;
                        }
                    },
                }
            }
            Ok(())
        }
    }

    fn process_events(events: Vec<Event>) -> Vec<EventBundle> {
        let mut result = Vec::new();
        let mut current_chars = Vec::new();
        let mut current_vertical = (0, 0);
        let mut current_horizontal = (0, 0);
        let mut current_others: Option<(Event, usize)> = None;

        for event in events {
            if let Some(ch) = Self::extract_char(&event) {
                Self::flush_non_char_buffers(
                    &mut result,
                    &mut current_vertical,
                    &mut current_horizontal,
                    &mut current_others,
                );
                current_chars.push(ch);
            } else if let Some((up, down)) = Self::detect_vertical_direction(&event) {
                Self::flush_char_buffer(&mut result, &mut current_chars);
                Self::flush_horizontal_buffer(&mut result, &mut current_horizontal);
                Self::flush_others_buffer(&mut result, &mut current_others);
                current_vertical.0 += up;
                current_vertical.1 += down;
            } else if let Some((left, right)) = Self::detect_horizontal_direction(&event) {
                Self::flush_char_buffer(&mut result, &mut current_chars);
                Self::flush_vertical_buffer(&mut result, &mut current_vertical);
                Self::flush_others_buffer(&mut result, &mut current_others);
                current_horizontal.0 += left;
                current_horizontal.1 += right;
            } else {
                Self::flush_char_buffer(&mut result, &mut current_chars);
                Self::flush_vertical_buffer(&mut result, &mut current_vertical);
                Self::flush_horizontal_buffer(&mut result, &mut current_horizontal);

                match &mut current_others {
                    Some((last_event, count)) if last_event == &event => {
                        *count += 1;
                    }
                    _ => {
                        Self::flush_others_buffer(&mut result, &mut current_others);
                        current_others = Some((event.clone(), 1));
                    }
                }
            }
        }

        // Flush remaining buffers
        Self::flush_char_buffer(&mut result, &mut current_chars);
        Self::flush_vertical_buffer(&mut result, &mut current_vertical);
        Self::flush_horizontal_buffer(&mut result, &mut current_horizontal);
        Self::flush_others_buffer(&mut result, &mut current_others);

        result
    }

    fn flush_char_buffer(result: &mut Vec<EventBundle>, chars: &mut Vec<char>) {
        if !chars.is_empty() {
            result.push(EventBundle::KeyBuffer(chars.clone()));
            chars.clear();
        }
    }

    fn flush_vertical_buffer(result: &mut Vec<EventBundle>, vertical: &mut (usize, usize)) {
        if *vertical != (0, 0) {
            result.push(EventBundle::VerticalCursorBuffer(vertical.0, vertical.1));
            *vertical = (0, 0);
        }
    }

    fn flush_horizontal_buffer(result: &mut Vec<EventBundle>, horizontal: &mut (usize, usize)) {
        if *horizontal != (0, 0) {
            result.push(EventBundle::HorizontalCursorBuffer(
                horizontal.0,
                horizontal.1,
            ));
            *horizontal = (0, 0);
        }
    }

    fn flush_others_buffer(result: &mut Vec<EventBundle>, others: &mut Option<(Event, usize)>) {
        if let Some((event, count)) = others.take() {
            result.push(EventBundle::Others(event, count));
        }
    }

    fn flush_non_char_buffers(
        result: &mut Vec<EventBundle>,
        vertical: &mut (usize, usize),
        horizontal: &mut (usize, usize),
        others: &mut Option<(Event, usize)>,
    ) {
        Self::flush_vertical_buffer(result, vertical);
        Self::flush_horizontal_buffer(result, horizontal);
        Self::flush_others_buffer(result, others);
    }

    fn extract_char(event: &Event) -> Option<char> {
        match event {
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
            }) => Some(*ch),
            _ => None,
        }
    }

    fn detect_vertical_direction(event: &Event) -> Option<(usize, usize)> {
        match event {
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => Some((1, 0)),
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            }) => Some((0, 1)),
            _ => None,
        }
    }

    fn detect_horizontal_direction(event: &Event) -> Option<(usize, usize)> {
        match event {
            Event::Key(KeyEvent {
                code: KeyCode::Left,
                ..
            }) => Some((1, 0)),
            Event::Key(KeyEvent {
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
                Event::Key(KeyEvent {
                    code: KeyCode::Char('a'),
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Char('B'),
                    modifiers: KeyModifiers::SHIFT,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Up,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Down,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Up,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Left,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Right,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Left,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Char('f'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Char('f'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Char('f'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Char('d'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Up,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                Event::Key(KeyEvent {
                    code: KeyCode::Char('d'),
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
            ];

            let expected = vec![
                EventBundle::KeyBuffer(vec!['a', 'B', 'c']),
                EventBundle::VerticalCursorBuffer(2, 1),
                EventBundle::HorizontalCursorBuffer(2, 1),
                EventBundle::Others(
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('f'),
                        modifiers: KeyModifiers::CONTROL,
                        kind: KeyEventKind::Press,
                        state: KeyEventState::NONE,
                    }),
                    3,
                ),
                EventBundle::Others(
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('d'),
                        modifiers: KeyModifiers::CONTROL,
                        kind: KeyEventKind::Press,
                        state: KeyEventState::NONE,
                    }),
                    1,
                ),
                EventBundle::VerticalCursorBuffer(1, 0),
                EventBundle::KeyBuffer(vec!['d']),
            ];

            assert_eq!(EventBuffer::process_events(events), expected);
        }

        #[test]
        fn test_only_others() {
            let events = vec![Event::Key(KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            })];

            let expected = vec![EventBundle::Others(
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }),
                1,
            )];

            assert_eq!(EventBuffer::process_events(events), expected);
        }
    }
}
