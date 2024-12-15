use async_trait::async_trait;
use promkit::pane::Pane;
use promkit::terminal::Terminal;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::{PaneIndex, PANESIZE};

#[derive(PartialEq)]
enum State {
    Idle,
    Loading,
    ProcessQuery,
}

#[async_trait]
pub trait Loader: Send + Sync + 'static {
    async fn load(item: &'static str) -> anyhow::Result<impl Visualizer>;
}

#[async_trait]
pub trait Visualizer: Send + Sync + 'static {
    async fn create_init_pane(&mut self, area: (u16, u16)) -> Pane;
    async fn create_pane_from(&mut self, area: (u16, u16), query: String) -> Pane;
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

    pub fn spawn_spin_task(
        &self,
        spin_panes: Arc<Mutex<[Pane]>>,
        spin_terminal: Arc<Mutex<Terminal>>,
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
                    let mut panes = spin_panes.lock().await;
                    let mut terminal = spin_terminal.lock().await;
                    panes[PaneIndex::Data as usize] = loading_pane;
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
        shared_visualizer: Arc<Mutex<impl Visualizer>>,
        shared_terminal: Arc<Mutex<Terminal>>,
        shared_panes: Arc<Mutex<[Pane; PANESIZE]>>,
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

                let mut visualizer = shared_visualizer.lock().await;
                visualizer.create_pane_from(area, query).await
            };

            {
                let mut panes = shared_panes.lock().await;
                let mut shared_state = shared.lock().await;
                let mut terminal = shared_terminal.lock().await;
                panes[PaneIndex::Data as usize] = result;
                shared_state.state = State::Idle;
                // TODO: error handling
                terminal.draw(&*panes);
            }
        })
    }

    pub async fn loading<L: Loader>(&self, item: &'static str) -> anyhow::Result<impl Visualizer> {
        {
            let mut shared_state = self.shared.lock().await;
            if let Some(task) = shared_state.current_task.take() {
                task.abort();
            }
            shared_state.state = State::Loading;
        }

        let visualizer = L::load(item).await;

        {
            let mut shared_state = self.shared.lock().await;
            shared_state.state = State::Idle;
        }

        visualizer
    }

    pub async fn write_on_resize(
        &self,
        shared_visualizer: Arc<Mutex<impl Visualizer>>,
        area: (u16, u16),
        query: String,
        shared_terminal: Arc<Mutex<Terminal>>,
        shared_panes: Arc<Mutex<[Pane; PANESIZE]>>,
    ) {
        {
            let mut shared_state = self.shared.lock().await;
            shared_state.area = area;
            if let Some(task) = shared_state.current_task.take() {
                task.abort();
            }
        }

        let process_task =
            self.spawn_process_task(query, shared_visualizer, shared_terminal, shared_panes);

        {
            let mut shared_state = self.shared.lock().await;
            shared_state.current_task = Some(process_task);
        }
    }

    pub async fn evaluate(
        &self,
        shared_visualizer: Arc<Mutex<impl Visualizer>>,
        query: String,
        shared_terminal: Arc<Mutex<Terminal>>,
        shared_panes: Arc<Mutex<[Pane; PANESIZE]>>,
    ) {
        {
            let mut shared_state = self.shared.lock().await;
            if let Some(task) = shared_state.current_task.take() {
                task.abort();
            }
        }

        let process_task =
            self.spawn_process_task(query, shared_visualizer, shared_terminal, shared_panes);

        {
            let mut shared_state = self.shared.lock().await;
            shared_state.current_task = Some(process_task);
        }
    }
}
