use async_trait::async_trait;
use promkit::pane::Pane;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Clone, PartialEq)]
enum State {
    Idle,
    ProcessQuery,
    ProcessEvents,
}

struct LoadingState {
    frame_index: usize,
    state: State,
}

#[async_trait]
pub trait Evaluator: Send + Sync + 'static {
    async fn process_query(&mut self, area: (u16, u16), query: String) -> Pane;
}

const LOADING_FRAMES: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub async fn evaluate(
    evaluator: impl Evaluator,
    area: (u16, u16),
    mut query_rx: mpsc::Receiver<String>,
    tx: mpsc::Sender<Pane>,
) {
    let shared_self = Arc::new(Mutex::new(evaluator));
    let mut current_task: Option<JoinHandle<()>> = None;
    let loading_state = Arc::new(Mutex::new(LoadingState {
        frame_index: 0,
        state: State::Idle,
    }));

    let loading_task = {
        let loading_state = loading_state.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            loop {
                interval.tick().await;

                let mut state = loading_state.lock().await;
                if state.state == State::Idle {
                    continue;
                }

                let frame_index = state.frame_index;
                state.frame_index = (state.frame_index + 1) % LOADING_FRAMES.len();
                drop(state);

                let loading_pane = Pane::new(
                    vec![promkit::grapheme::StyledGraphemes::from(
                        LOADING_FRAMES[frame_index],
                    )],
                    0,
                );
                if tx.send(loading_pane).await.is_err() {
                    break;
                }
            }
        })
    };

    loop {
        tokio::select! {
            Some(query) = query_rx.recv() => {
                if let Some(task) = current_task.take() {
                    task.abort();
                }

                let this = shared_self.clone();
                let tx = tx.clone();
                let loading_state = loading_state.clone();

                let process_task = tokio::spawn(async move {
                    {
                        let mut state = loading_state.lock().await;
                        state.state = State::ProcessQuery;
                    }

                    let result = {
                        let mut evaluator = this.lock().await;
                        evaluator.process_query(area, query).await
                    };

                    let _ = tx.send(result).await;

                    let mut state = loading_state.lock().await;
                    state.state = State::Idle;
                });

                current_task = Some(process_task);
            }
            else => {
                loading_task.abort();
                break;
            }
        }
    }
}
