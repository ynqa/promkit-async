use async_trait::async_trait;
use promkit::pane::Pane;
use tokio::sync::mpsc::{Receiver, Sender};

#[async_trait]
pub trait InputProcessor<I: Clone + Send + Sync + 'static> {
    fn process_event(&mut self, area: (u16, u16), inputs: I) -> Pane;

    async fn run(&mut self, area: (u16, u16), mut rx: Receiver<I>, tx: Sender<Pane>) {
        while let Some(inputs) = rx.recv().await {
            let pane = self.process_event(area, inputs);
            if tx.send(pane).await.is_err() {
                break;
            }
        }
    }
}
