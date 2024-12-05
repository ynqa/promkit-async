use async_trait::async_trait;
use promkit::pane::Pane;
use tokio::sync::mpsc;

use crate::EventGroup;

pub mod loading;
pub use loading::LoadingComponent;

#[async_trait]
pub trait Component: Send + Sync + 'static {
    async fn run(
        &mut self,
        area: (u16, u16),
        rx: mpsc::Receiver<Vec<EventGroup>>,
        tx: mpsc::Sender<Pane>,
    );
}
