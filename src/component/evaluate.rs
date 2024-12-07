use async_trait::async_trait;
use promkit::pane::Pane;
use tokio::sync::mpsc;

use crate::EventGroup;

enum State {
    Idle,
    ProcessQuery,
    ProcessEvents,
}

#[async_trait]
pub trait Evaluator: Clone + Send + Sync + 'static {
    const LOADING_FRAMES: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    async fn process_query(&mut self, area: (u16, u16), query: String) -> Pane;
    async fn process_events(&mut self, area: (u16, u16), events: Vec<EventGroup>) -> Pane;
    async fn rollback_state(&mut self) -> bool;
    async fn run(
        &mut self,
        area: (u16, u16),
        query_rx: mpsc::Receiver<String>,
        events_rx: mpsc::Receiver<Vec<EventGroup>>,
        tx: mpsc::Sender<Pane>,
    ) {
    }
}
