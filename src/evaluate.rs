use async_trait::async_trait;
use promkit::pane::Pane;
use promkit::terminal::Terminal;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Clone, PartialEq)]
enum State {
    Idle,
    ProcessQuery,
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
    shared_terminal: Arc<Mutex<Terminal>>,
    shared_panes: Arc<Mutex<[Pane; 2]>>,
    spin_duration: Duration,
) {
    let shared_self = Arc::new(Mutex::new(evaluator));
    let mut current_task: Option<JoinHandle<()>> = None;
    let loading_state = Arc::new(Mutex::new(LoadingState {
        frame_index: 0,
        state: State::Idle,
    }));

    let loading_panes = shared_panes.clone();
    let loading_terminal = shared_terminal.clone();

    let loading_task = {
        let loading_state = loading_state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(spin_duration);
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
                {
                    let mut panes = loading_panes.lock().await;
                    let mut terminal = loading_terminal.lock().await;
                    panes[1] = loading_pane;
                    // TODO: error handling
                    terminal.draw(&*panes);
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
                let loading_state = loading_state.clone();
                let evaluating_panes = shared_panes.clone();
                let evaluating_terminal = shared_terminal.clone();

                let process_task = tokio::spawn(async move {
                    {
                        let mut state = loading_state.lock().await;
                        state.state = State::ProcessQuery;
                    }

                    let result = {
                        let mut evaluator = this.lock().await;
                        evaluator.process_query(area, query).await
                    };

                    {
                        let mut panes = evaluating_panes.lock().await;
                        let mut state = loading_state.lock().await;
                        let mut terminal = evaluating_terminal.lock().await;
                        panes[1] = result;
                        state.state = State::Idle;
                        // TODO: error handling
                        terminal.draw(&*panes);
                    }
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
