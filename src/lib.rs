use std::{io, pin::Pin, time::Duration};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use futures::stream::Stream;
use futures::StreamExt;
use promkit::{
    crossterm::{
        cursor,
        event::EventStream,
        execute,
        terminal::{disable_raw_mode, enable_raw_mode},
    },
    grapheme::StyledGraphemes,
    pane::Pane,
    terminal::Terminal,
};
use tokio::sync::mpsc;

pub mod component;
pub mod event;
pub use event::EventGroup;
pub mod operator;
use operator::TimeBasedOperator;
pub mod snapshot;

pub struct Prompt {}

impl Drop for Prompt {
    fn drop(&mut self) {
        execute!(io::stdout(), cursor::MoveToNextLine(1), cursor::Show).ok();
        disable_raw_mode().ok();
    }
}

impl Prompt {
    pub async fn run(
        &mut self,
        senders: Vec<mpsc::Sender<Vec<EventGroup>>>,
        receivers: Vec<mpsc::Receiver<Pane>>,
        delay: Duration,
    ) -> anyhow::Result<()> {
        enable_raw_mode()?;
        execute!(io::stdout(), cursor::Hide)?;

        let mut operator = TimeBasedOperator {};
        let (event_sender, event_receiver) = mpsc::channel(1);
        let (event_group_sender, mut event_group_receiver) = mpsc::channel(1);

        let operator_handle = tokio::spawn(async move {
            operator
                .run(delay, event_receiver, event_group_sender)
                .await
        });

        let mut panes: Vec<Pane> = (0..receivers.len())
            .map(|_| Pane::new(vec![StyledGraphemes::from("")], 0))
            .collect();

        let pane_stream = futures::stream::select_all(
            receivers
                .into_iter()
                .enumerate()
                .map(|(index, rx)| {
                    Box::pin(
                        futures::stream::unfold(rx, move |mut rx| async move {
                            rx.recv().await.map(|pane| (pane, rx))
                        })
                        .map(move |pane| (pane, index)),
                    ) as Pin<Box<dyn Stream<Item = (Pane, usize)> + Send>>
                })
                .collect::<Vec<_>>(),
        );
        tokio::pin!(pane_stream);

        let mut terminal = Terminal {
            position: cursor::position()?,
        };
        let mut stream = EventStream::new();
        let mut result = Ok(());

        'main: loop {
            tokio::select! {
                Some(Ok(event)) = stream.next() => {
                    if event == Event::Key(KeyEvent {
                        code: KeyCode::Esc,
                        modifiers: KeyModifiers::NONE,
                        kind: KeyEventKind::Press,
                        state: KeyEventState::NONE,
                    }) {
                        break 'main;
                    }
                    if let Err(e) = event_sender.send(event).await {
                        result = Err(anyhow::anyhow!("Failed to send event: {}", e));
                        break 'main;
                    }
                },
                Some(event_groups) = event_group_receiver.recv() => {
                    for sender in &senders {
                        if let Err(e) = sender.send(event_groups.clone()).await {
                            result = Err(anyhow::anyhow!("Failed to send event groups: {}", e));
                            break 'main;
                        }
                    }
                },
                Some((pane, index)) = pane_stream.next() => {
                    panes[index] = pane;
                    if let Err(e) = terminal.draw(&panes) {
                        result = Err(anyhow::anyhow!("Failed to draw panes: {}", e));
                        break 'main;
                    }
                },
                else => {
                    break 'main;
                }
            }
        }

        operator_handle.abort();

        result
    }
}
