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
pub use wizard::{Context, Processor, Spinner, Wizard};

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
        processor: impl Processor,
        spin_duration: Duration,
        query_debounce_duration: Duration,
        resize_debounce_duration: Duration,
    ) -> anyhow::Result<()> {
        enable_raw_mode()?;
        execute!(io::stdout(), cursor::Hide)?;

        let size = terminal::size()?;
        let mut editor = Editor::default();

        let shared_terminal = Arc::new(Mutex::new(Terminal {
            position: cursor::position()?,
        }));
        let shared_panes: Arc<Mutex<[Pane; 2]>> = Arc::new(Mutex::new([
            editor.create_pane(size.0, size.1),
            Pane::new(vec![StyledGraphemes::from(" ")], 0),
        ]));

        let (last_query_tx, mut last_query_rx) = mpsc::channel(1);
        let (debounce_query_tx, debounce_query_rx) = mpsc::channel(1);
        let query_debouncer =
            spawn_debouncer(debounce_query_rx, last_query_tx, query_debounce_duration);

        let (last_resize_tx, mut last_resize_rx) = mpsc::channel(1);
        let (debounce_resize_tx, debounce_resize_rx) = mpsc::channel(1);
        let resize_debouncer =
            spawn_debouncer(debounce_resize_rx, last_resize_tx, resize_debounce_duration);

        let ctx = Arc::new(Mutex::new(Context::new(size)));

        let spinner = Spinner::new(ctx.clone());
        let (spinner_panes, spinner_terminal) = (shared_panes.clone(), shared_terminal.clone());
        let spinning = tokio::spawn(async move {
            spinner
                .spawn_loading_task(spinner_panes, spinner_terminal, spin_duration)
                .await
        });

        let wiz = Wizard::new(ctx.clone());
        let shared_evaluator = Arc::new(Mutex::new(processor));

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
                Some(query) = last_query_rx.recv() => {
                    wiz.evaluate(shared_evaluator.clone(), query, shared_terminal.clone(), shared_panes.clone()).await;
                }
                Some(area) = last_resize_rx.recv() => {
                    let pane = editor.create_pane(area.0, area.1);
                    {
                        let mut panes = shared_panes.lock().await;
                        let mut terminal = shared_terminal.lock().await;
                        panes[0] = pane;
                        terminal.draw(&*panes)?;
                    }
                    wiz.write_on_resize(shared_evaluator.clone(), area, editor.text(), shared_terminal.clone(), shared_panes.clone()).await;
                }
                else => {
                    break 'main;
                }
            }
        }

        spinning.abort();
        query_debouncer.abort();
        resize_debouncer.abort();

        Ok(())
    }
}
