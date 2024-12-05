use async_trait::async_trait;
use promkit::pane::Pane;
use tokio::sync::mpsc::{Receiver, Sender};

use crate::EventGroup;

#[async_trait]
pub trait SyncComponent {
    fn process_event(&mut self, area: (u16, u16), event_groups: &Vec<EventGroup>) -> Pane;

    async fn run(&mut self, area: (u16, u16), mut rx: Receiver<Vec<EventGroup>>, tx: Sender<Pane>) {
        while let Some(event_groups) = rx.recv().await {
            let pane = self.process_event(area, &event_groups);
            if tx.send(pane).await.is_err() {
                break;
            }
        }
    }
}
