use std::{io, pin::Pin, time::Duration};

use component::Component;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use futures::stream::Stream;
use futures::{
    stream::{SelectAll, StreamExt},
    SinkExt,
};
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
    components: Vec<Box<dyn Component>>,
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
            Vec<mpsc::Sender<&Vec<EventGroup>>>,
            Vec<mpsc::Receiver<&Vec<EventGroup>>>,
        ) = self.components.iter().map(|_| mpsc::channel(1)).unzip();

        for (component, receiver) in self.components.iter().zip(event_receivers) {
            tokio::spawn(async move { component.subscribe(receiver) });
        }

        let (pane_senders, pane_receivers): (Vec<mpsc::Sender<&Pane>>, Vec<mpsc::Receiver<&Pane>>) =
            self.components.iter().map(|_| mpsc::channel(1)).unzip();

        for (component, sender) in self.components.iter().zip(pane_senders) {
            tokio::spawn(async move { component.publish(sender) });
        }

        let mut pane_stream = Box::pin(futures::stream::select_all(
            pane_receivers
                .iter()
                .enumerate()
                .map(|(index, receiver)| {
                    let stream = Box::pin(futures::stream::unfold(receiver, |mut rx| async move {
                        rx.recv().await.map(|pane| (pane, rx))
                    }));
                    let mapped_stream = stream.map(move |pane| (pane, index));
                    Box::pin(mapped_stream) as Pin<Box<dyn Stream<Item = (&Pane, usize)> + Send>>
                })
                .collect(),
        ));

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
                Some(event_group) = event_group_receiver.recv() => {
                    for idx in 0..self.components.len() {
                        let _ = event_senders[idx].send(&event_group);
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
