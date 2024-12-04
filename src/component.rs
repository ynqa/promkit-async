use std::time::Duration;

use async_trait::async_trait;
use promkit::{grapheme::StyledGraphemes, pane::Pane};
use tokio::sync::mpsc;

use crate::EventGroup;

#[async_trait]
pub trait Component: Send + Sync + 'static {
    async fn subscribe(&self, events: mpsc::Receiver<Vec<EventGroup>>);
    async fn publish(&self, pane: mpsc::Sender<Pane>);
}

pub trait LoadingComponent: Component {
    const LOADING_FRAMES: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    async fn process_event(&mut self, event_group: &EventGroup) -> Pane;

    async fn handle_event(&mut self, event_group: &EventGroup) -> Pane {
        let (tx, mut rx) = mpsc::channel(1);

        tokio::select! {
            _ = async {
                let mut frame_index = 0;
                let mut interval = tokio::time::interval(Duration::from_millis(100));
                loop {
                    interval.tick().await;
                    let _ = tx.send(Pane::new(
                        vec![StyledGraphemes::from(Self::LOADING_FRAMES[frame_index])],
                        0,
                    )).await;
                    frame_index = (frame_index + 1) % Self::LOADING_FRAMES.len();
                }
            } => unreachable!(),
            result = self.process_event(event_group) => {
                drop(tx);
                result
            },
            Some(loading_pane) = rx.recv() => loading_pane,
        }
    }
}
