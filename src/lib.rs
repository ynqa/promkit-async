use std::{io, pin::Pin, sync::Arc, time::Duration};

use component::Component;
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
pub mod operator;
use operator::{EventGroup, TimeBasedOperator};

pub struct Prompt {
    components: Vec<Arc<dyn Component>>,
}

impl Drop for Prompt {
    fn drop(&mut self) {
        execute!(io::stdout(), cursor::MoveToNextLine(1), cursor::Show).ok();
        disable_raw_mode().ok();
    }
}

impl Prompt {
    pub async fn run(&mut self, delay: Duration) -> anyhow::Result<()> {
        enable_raw_mode()?;
        execute!(io::stdout(), cursor::Hide)?;

        let mut operator = TimeBasedOperator {};
        let (event_sender, event_receiver) = mpsc::channel(1);
        let (event_group_sender, mut event_group_receiver) = mpsc::channel(1);
        tokio::spawn(async move {
            operator
                .run(delay, event_receiver, event_group_sender)
                .await
        });

        let mut terminal = Terminal {
            position: cursor::position()?,
        };

        let mut stream = EventStream::new();

        let mut panes: Vec<Pane> = self
            .components
            .iter()
            .map(|_| Pane::new(vec![StyledGraphemes::from("hi")], 0))
            .collect();

        let (event_senders, event_receivers): (
            Vec<mpsc::Sender<Vec<EventGroup>>>,
            Vec<mpsc::Receiver<Vec<EventGroup>>>,
        ) = self.components.iter().map(|_| mpsc::channel(1)).unzip();

        let (pane_senders, pane_receivers): (Vec<mpsc::Sender<Pane>>, Vec<mpsc::Receiver<Pane>>) =
            self.components.iter().map(|_| mpsc::channel(1)).unzip();

        for ((component, event_rx), pane_tx) in self
            .components
            .iter()
            .zip(event_receivers)
            .zip(pane_senders)
        {
            let component = Arc::clone(component);
            tokio::spawn(async move {
                tokio::join!(component.subscribe(event_rx), component.publish(pane_tx));
            });
        }

        let pane_stream = futures::stream::select_all(
            pane_receivers
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

        loop {
            tokio::select! {
                Some(Ok(event)) = stream.next() => {
                    if event == Event::Key(KeyEvent {
                        code: KeyCode::Esc,
                        modifiers: KeyModifiers::NONE,
                        kind: KeyEventKind::Press,
                        state: KeyEventState::NONE,
                    }) {
                        return Ok(())
                    }
                    event_sender.send(event).await?;
                },
                Some(event_groups) = event_group_receiver.recv() => {
                    for sender in &event_senders {
                        let _ = sender.send(event_groups.clone()).await;
                    }
                },
                Some((pane, index)) = pane_stream.next() => {
                    panes[index] = pane;
                    terminal.draw(&panes)?;
                }
            }
        }
    }
}
