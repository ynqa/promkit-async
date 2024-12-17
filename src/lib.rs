use std::{io, sync::Arc};

use crossterm::{
    self, cursor,
    event::EventStream,
    execute,
    terminal::{self, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use promkit::{grapheme::StyledGraphemes, pane::Pane, terminal::Terminal};
use tokio::sync::{mpsc, Mutex};

mod editor;
pub use editor::Editor;
mod evaluate;
pub use evaluate::Evaluator;

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

impl Prompt {
    pub async fn run<E>(&mut self, evaluator: E) -> anyhow::Result<()>
    where
        E: Evaluator + Send + 'static,
    {
        enable_raw_mode()?;
        execute!(io::stdout(), cursor::Hide)?;

        let mut terminal = Terminal {
            position: cursor::position()?,
        };

        let size = terminal::size()?;
        let mut editor = Editor::default();
        let shared_panes: Arc<Mutex<[Pane; 2]>> = Arc::new(Mutex::new([
            editor.create_pane(size.0, size.1),
            Pane::new(vec![StyledGraphemes::from(" ")], 0),
        ]));

        let (query_tx, query_rx) = mpsc::channel(1);
        let (pane_tx, mut pane_rx) = mpsc::channel(1);

        let evaluator = Arc::new(Mutex::new(evaluator));
        let evaluator_clone = evaluator.clone();

        let evaluating = tokio::spawn(async move {
            let mut evaluator = evaluator_clone.lock().await;
            evaluator.run(size, query_rx, pane_tx).await
        });

        let mut stream = EventStream::new();

        'main: loop {
            tokio::select! {
                Some(Ok(event)) = stream.next() => {
                    match editor.evaluate(&event)? {
                        Action::None => {
                            continue 'main;
                        },
                        Action::Quit => {
                            break 'main;
                        }
                        Action::ChangeText => {
                            query_tx.send(editor.text()).await?;
                        }
                        Action::MoveCursor => (),
                    }
                    let size = terminal::size()?;
                    let pane = editor.create_pane(size.0, size.1);
                    {
                        let mut panes = shared_panes.lock().await;
                        panes[0] = pane;
                        terminal.draw(&*panes)?;
                    }
                },
                Some(pane) = pane_rx.recv() => {
                    {
                        let mut panes = shared_panes.lock().await;
                        panes[1] = pane;
                        terminal.draw(&*panes)?;
                    }
                },
                else => {
                    break 'main;
                }
            }
        }

        evaluating.abort();

        Ok(())
    }
}
