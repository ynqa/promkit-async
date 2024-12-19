use async_trait::async_trait;
use promkit::pane::Pane;
use promkit::terminal::Terminal;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[derive(Clone, PartialEq)]
enum State {
    Idle,
    ProcessQuery,
    RewriteOnResize,
}

#[derive(PartialEq)]
struct LoadingState {
    frame_index: usize,
    state: State,
}

#[async_trait]
pub trait Evaluator: Send + Sync + 'static {
    async fn process_query(&mut self, area: (u16, u16), query: String) -> Pane;
}

const LOADING_FRAMES: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct SharedState {
    loading_state: LoadingState,
    area: (u16, u16),
    current_task: Option<JoinHandle<()>>,
}

impl SharedState {
    pub fn new(area: (u16, u16)) -> Self {
        Self {
            loading_state: LoadingState {
                frame_index: 0,
                state: State::Idle,
            },
            area,
            current_task: None,
        }
    }
}

pub struct LoadingManager {
    shared: Arc<Mutex<SharedState>>,
}

impl LoadingManager {
    pub fn new(shared: Arc<Mutex<SharedState>>) -> Self {
        Self { shared }
    }

    pub fn spawn_loading_task(
        &self,
        loading_panes: Arc<Mutex<[Pane; 2]>>,
        loading_terminal: Arc<Mutex<Terminal>>,
        spin_duration: Duration,
    ) -> JoinHandle<()> {
        let shared = self.shared.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(spin_duration);
            loop {
                interval.tick().await;

                let mut shared_state = shared.lock().await;
                if shared_state.loading_state.state == State::Idle {
                    continue;
                }

                let frame_index = shared_state.loading_state.frame_index;
                shared_state.loading_state.frame_index = (frame_index + 1) % LOADING_FRAMES.len();
                drop(shared_state);

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
    }
}

pub struct QueryEvaluator {
    shared: Arc<Mutex<SharedState>>,
}

impl QueryEvaluator {
    pub fn new(shared: Arc<Mutex<SharedState>>) -> Self {
        Self { shared }
    }

    fn spawn_process_task(
        &self,
        query: String,
        shared_evaluator: Arc<Mutex<impl Evaluator>>,
        shared_panes: Arc<Mutex<[Pane; 2]>>,
        shared_terminal: Arc<Mutex<Terminal>>,
    ) -> JoinHandle<()> {
        let shared = self.shared.clone();
        tokio::spawn(async move {
            {
                let mut shared_state = shared.lock().await;
                shared_state.loading_state.state = State::ProcessQuery;
            }

            let result = {
                let shared_state = shared.lock().await;
                let area = shared_state.area;
                drop(shared_state);

                let mut evaluator = shared_evaluator.lock().await;
                evaluator.process_query(area, query).await
            };

            {
                let mut panes = shared_panes.lock().await;
                let mut shared_state = shared.lock().await;
                let mut terminal = shared_terminal.lock().await;
                panes[1] = result;
                shared_state.loading_state.state = State::Idle;
                // TODO: error handling
                terminal.draw(&*panes);
            }
        })
    }

    pub async fn evaluate(
        &self,
        shared_evaluator: Arc<Mutex<impl Evaluator>>,
        query: String,
        shared_terminal: Arc<Mutex<Terminal>>,
        shared_panes: Arc<Mutex<[Pane; 2]>>,
    ) {
        {
            let mut shared_state = self.shared.lock().await;
            if let Some(task) = shared_state.current_task.take() {
                task.abort();
            }
        }

        let process_task =
            self.spawn_process_task(query, shared_evaluator, shared_panes, shared_terminal);

        {
            let mut shared_state = self.shared.lock().await;
            shared_state.current_task = Some(process_task);
        }
    }
}
