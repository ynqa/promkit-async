use async_trait::async_trait;
use promkit::pane::Pane;
use promkit::terminal::Terminal;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[derive(PartialEq)]
enum State {
    Idle,
    ProcessQuery,
    Rewrite,
}

#[async_trait]
pub trait Processor: Send + Sync + 'static {
    async fn process_query(&mut self, area: (u16, u16), query: String) -> Pane;
}

const LOADING_FRAMES: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct Context {
    state: State,
    area: (u16, u16),
    current_task: Option<JoinHandle<()>>,
}

impl Context {
    pub fn new(area: (u16, u16)) -> Self {
        Self {
            state: State::Idle,
            area,
            current_task: None,
        }
    }
}

pub struct Spinner {
    shared: Arc<Mutex<Context>>,
}

impl Spinner {
    pub fn new(shared: Arc<Mutex<Context>>) -> Self {
        Self { shared }
    }

    pub fn spawn_loading_task(
        &self,
        loading_panes: Arc<Mutex<[Pane; 2]>>,
        loading_terminal: Arc<Mutex<Terminal>>,
        spin_duration: Duration,
    ) -> JoinHandle<()> {
        let shared = self.shared.clone();
        let mut frame_index = 0;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(spin_duration);
            loop {
                interval.tick().await;

                {
                    let shared_state = shared.lock().await;
                    if shared_state.state == State::Idle {
                        continue;
                    }
                }

                frame_index = (frame_index + 1) % LOADING_FRAMES.len();

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

pub struct Wizard {
    shared: Arc<Mutex<Context>>,
}

impl Wizard {
    pub fn new(shared: Arc<Mutex<Context>>) -> Self {
        Self { shared }
    }

    fn spawn_process_task(
        &self,
        query: String,
        shared_processor: Arc<Mutex<impl Processor>>,
        shared_panes: Arc<Mutex<[Pane; 2]>>,
        shared_terminal: Arc<Mutex<Terminal>>,
    ) -> JoinHandle<()> {
        let shared = self.shared.clone();
        tokio::spawn(async move {
            {
                let mut shared_state = shared.lock().await;
                shared_state.state = State::ProcessQuery;
            }

            let result = {
                let shared_state = shared.lock().await;
                let area = shared_state.area;
                drop(shared_state);

                let mut processor = shared_processor.lock().await;
                processor.process_query(area, query).await
            };

            {
                let mut panes = shared_panes.lock().await;
                let mut shared_state = shared.lock().await;
                let mut terminal = shared_terminal.lock().await;
                panes[1] = result;
                shared_state.state = State::Idle;
                // TODO: error handling
                terminal.draw(&*panes);
            }
        })
    }

    pub async fn write_on_resize(
        &self,
        shared_processor: Arc<Mutex<impl Processor>>,
        area: (u16, u16),
        query: String,
        shared_terminal: Arc<Mutex<Terminal>>,
        shared_panes: Arc<Mutex<[Pane; 2]>>,
    ) {
        {
            let mut shared_state = self.shared.lock().await;
            shared_state.area = area;
            if let Some(task) = shared_state.current_task.take() {
                task.abort();
            }
        }

        let process_task =
            self.spawn_process_task(query, shared_processor, shared_panes, shared_terminal);

        {
            let mut shared_state = self.shared.lock().await;
            shared_state.current_task = Some(process_task);
        }
    }

    pub async fn evaluate(
        &self,
        shared_processor: Arc<Mutex<impl Processor>>,
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
            self.spawn_process_task(query, shared_processor, shared_panes, shared_terminal);

        {
            let mut shared_state = self.shared.lock().await;
            shared_state.current_task = Some(process_task);
        }
    }
}
