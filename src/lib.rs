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
    fn merge_pane_streams(&self) -> Pin<Box<SelectAll<impl Stream<Item = (Pane, usize)>>>> {
        let streams: Vec<Pin<Box<dyn Stream<Item = (Pane, usize)> + Send>>> = self
            .components
            .iter()
            .enumerate()
            .map(|(index, component)| {
                let receiver = component.pane_receiver();
                let stream = Box::pin(futures::stream::unfold(receiver, |mut rx| async move {
                    rx.next().await.map(|pane| (pane, rx))
                }));
                let mapped_stream = stream.map(move |pane| (pane, index));
                Box::pin(mapped_stream) as Pin<Box<dyn Stream<Item = (Pane, usize)> + Send>>
            })
            .collect();

        Box::pin(futures::stream::select_all(streams))
    }

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

        let mut pane_stream = self.merge_pane_streams();
        let mut stream = EventStream::new();

        let mut panes = self
            .components
            .iter()
            .map(|_| Pane::new(vec![StyledGraphemes::from("hi")], 0))
            .collect::<Vec<Pane>>();

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
                    for component in self.components.iter_mut() {
                        let _ = component.event_group_sender().send(&event_group);
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
