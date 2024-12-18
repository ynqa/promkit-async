use std::{io, sync::Arc, time::Duration};

use crossterm::{
    self, cursor,
    event::{Event, EventStream},
    execute,
    terminal::{self, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use futures_timer::Delay;
use promkit::{grapheme::StyledGraphemes, pane::Pane, terminal::Terminal};
use tokio::sync::{mpsc, Mutex};

mod editor;
pub use editor::Editor;
mod wizard;
pub use wizard::{Evaluator, Wizard};

pub struct Prompt {}

impl Drop for Prompt {
    fn drop(&mut self) {
        execute!(io::stdout(), cursor::MoveToNextLine(1), cursor::Show).ok();
        disable_raw_mode().ok();
    }
}

pub enum Action {
    None,
    MoveCursor,
    ChangeText,
    Quit,
}

fn spawn_debouncer<T: Send + 'static>(
    mut debounce_rx: mpsc::Receiver<T>,
    last_tx: mpsc::Sender<T>,
    duration: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_query = None;
        loop {
            let delay = Delay::new(duration);
            futures::pin_mut!(delay);

            tokio::select! {
                maybe_query = debounce_rx.recv() => {
                    if let Some(query) = maybe_query {
                        last_query = Some(query);
                    } else {
                        break;
                    }
                },
                _ = delay => {
                    if let Some(text) = last_query.take() {
                        let _ = last_tx.send(text).await;
                    }
                },
            }
        }
    })
}

impl Prompt {
    pub async fn run(
        &mut self,
        evaluator: impl Evaluator,
        query_debounce_duration: Duration,
        spin_duration: Duration,
    ) -> anyhow::Result<()> {
        enable_raw_mode()?;
        execute!(io::stdout(), cursor::Hide)?;

        let shared_terminal = Arc::new(Mutex::new(Terminal {
            position: cursor::position()?,
        }));

        let size = terminal::size()?;
        let mut editor = Editor::default();
        let shared_panes: Arc<Mutex<[Pane; 2]>> = Arc::new(Mutex::new([
            editor.create_pane(size.0, size.1),
            Pane::new(vec![StyledGraphemes::from(" ")], 0),
        ]));

        let (last_query_tx, last_query_rx) = mpsc::channel(1);
        let (debounce_query_tx, debounce_query_rx) = mpsc::channel(1);
        let query_debouncer =
            spawn_debouncer(debounce_query_rx, last_query_tx, query_debounce_duration);

        let (last_resize_tx, mut last_resize_rx) = mpsc::channel(1);
        let (debounce_resize_tx, debounce_resize_rx) = mpsc::channel(1);
        let resize_debouncer =
            spawn_debouncer(debounce_resize_rx, last_resize_tx, query_debounce_duration);

        let evaluating_panes = shared_panes.clone();
        let evaluating_terminal = shared_terminal.clone();
        let wizard = Wizard::new(size);
        let evaluating = tokio::spawn(async move {
            wizard
                .evaluate(
                    evaluator,
                    last_query_rx,
                    evaluating_terminal,
                    evaluating_panes,
                    spin_duration,
                )
                .await
        });

        let mut stream = EventStream::new();

        'main: loop {
            tokio::select! {
                Some(Ok(event)) = stream.next() => {
                    if let Event::Resize(width, height) = event {
                        debounce_resize_tx.send((width, height)).await?;
                    } else {
                        match editor.evaluate(&event)? {
                            Action::None => {
                                continue 'main;
                            },
                            Action::Quit => {
                                break 'main;
                            }
                            Action::ChangeText => {
                                debounce_query_tx.send(editor.text()).await?;
                            }
                            Action::MoveCursor => (),
                        }
                        let size = terminal::size()?;
                        let pane = editor.create_pane(size.0, size.1);
                        {
                            let mut panes = shared_panes.lock().await;
                            let mut terminal = shared_terminal.lock().await;
                            panes[0] = pane;
                            terminal.draw(&*panes)?;
                        }
                    }
                },
                Some(area) = last_resize_rx.recv() => {
                    let pane = editor.create_pane(size.0, size.1);

                    {
                        let mut panes = shared_panes.lock().await;
                        let mut terminal = shared_terminal.lock().await;
                        panes[0] = pane;
                        terminal.draw(&*panes)?;
                    }
                }
                else => {
                    break 'main;
                }
            }
        }

        evaluating.abort();
        query_debouncer.abort();
        resize_debouncer.abort();

        Ok(())
    }
}
