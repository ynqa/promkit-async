use std::time::Duration;

use async_trait::async_trait;
use promkit::{grapheme::StyledGraphemes, pane::Pane};
use tokio::sync::mpsc;

use crate::EventGroup;

#[async_trait]
pub trait Component: Send + Sync + 'static {
    async fn run(&mut self, mut rx: mpsc::Receiver<Vec<EventGroup>>, tx: mpsc::Sender<Pane>);
}

#[async_trait]
pub trait LoadingComponent: Component {
    const LOADING_FRAMES: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    async fn process_event(&mut self, event_group: &EventGroup) -> Pane;

    async fn run_loading(
        &mut self,
        event_group: &EventGroup,
        cancel_rx: &mut mpsc::Receiver<()>,
    ) -> Option<Pane> {
        let (loading_tx, mut loading_rx): (mpsc::Sender<Pane>, mpsc::Receiver<Pane>) =
            mpsc::channel(1);

        tokio::select! {
            _ = async {
                let mut frame_index = 0;
                let mut interval = tokio::time::interval(Duration::from_millis(100));
                loop {
                    interval.tick().await;
                    let _ = loading_tx.send(Pane::new(
                        vec![StyledGraphemes::from(Self::LOADING_FRAMES[frame_index])],
                        0,
                    )).await;
                    frame_index = (frame_index + 1) % Self::LOADING_FRAMES.len();
                }
            } => unreachable!(),
            result = self.process_event(event_group) => {
                drop(loading_tx);
                Some(result)
            },
            Some(loading_pane) = loading_rx.recv() => Some(loading_pane),
            _ = cancel_rx.recv() => {
                drop(loading_tx);
                None
            }
        }
    }

    async fn run(&mut self, mut rx: mpsc::Receiver<Vec<EventGroup>>, tx: mpsc::Sender<Pane>) {
        let mut current_cancel_tx: Option<mpsc::Sender<()>> = None;

        while let Some(event_groups) = rx.recv().await {
            for event_group in event_groups {
                if let Some(cancel_tx) = current_cancel_tx.take() {
                    let _ = cancel_tx.send(()).await;
                }

                let (new_cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);
                current_cancel_tx = Some(new_cancel_tx);

                loop {
                    if let Some(pane) = self.run_loading(&event_group, &mut cancel_rx).await {
                        if tx.send(pane).await.is_err() {
                            return;
                        }
                        break;
                    }
                }
            }
        }
    }
}
