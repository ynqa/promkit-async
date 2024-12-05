use std::{future::Future, sync::Arc, time::Duration};

use async_trait::async_trait;
use promkit::{grapheme::StyledGraphemes, pane::Pane};
use tokio::{
    sync::{mpsc, Mutex},
    task::JoinHandle,
};

use crate::EventGroup;

pub struct StateHistory<T> {
    inner: Arc<Mutex<StateHistoryInner<T>>>,
}

struct StateHistoryInner<T> {
    current: T,
    previous: Option<T>,
}

impl<T: Clone + Send + Sync + 'static> StateHistory<T> {
    pub fn new(initial: T) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StateHistoryInner {
                current: initial,
                previous: None,
            })),
        }
    }

    pub async fn update(&self, new_state: T) {
        let mut inner = self.inner.lock().await;
        inner.previous = Some(inner.current.clone());
        inner.current = new_state;
    }

    pub async fn rollback(&self) -> bool {
        let mut inner = self.inner.lock().await;
        if let Some(prev) = inner.previous.take() {
            inner.current = prev;
            true
        } else {
            false
        }
    }

    pub async fn modify<F, Fut, R>(&self, f: F) -> R
    where
        F: FnOnce(T) -> Fut + Send,
        Fut: Future<Output = (T, R)> + Send,
        R: Send,
        T: Clone + Send,
    {
        let current = {
            let inner = self.inner.lock().await;
            inner.current.clone()
        };

        let (new_state, result) = f(current).await;
        self.update(new_state).await;
        result
    }
}

impl<T: Clone + Send + Sync + 'static> Clone for StateHistory<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[async_trait]
pub trait Component: Send + Sync + 'static {
    async fn run(
        &mut self,
        area: (u16, u16),
        mut rx: mpsc::Receiver<Vec<EventGroup>>,
        tx: mpsc::Sender<Pane>,
    );
}

struct LoadingState {
    is_loading: bool,
    frame_index: usize,
}

#[async_trait]
pub trait LoadingComponent: Component + Clone {
    const LOADING_FRAMES: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    async fn process_event(&mut self, area: (u16, u16), event_groups: &Vec<EventGroup>) -> Pane;

    async fn rollback_state(&mut self) -> bool;

    async fn run(
        &mut self,
        area: (u16, u16),
        mut rx: mpsc::Receiver<Vec<EventGroup>>,
        tx: mpsc::Sender<Pane>,
    ) {
        let mut current_task: Option<JoinHandle<Result<(), mpsc::error::SendError<Pane>>>> = None;
        let loading_state = Arc::new(Mutex::new(LoadingState {
            is_loading: false,
            frame_index: 0,
        }));

        let loading_task = {
            let loading_state = loading_state.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_millis(100));
                loop {
                    interval.tick().await;

                    let mut state = loading_state.lock().await;
                    if !state.is_loading {
                        continue;
                    }

                    let frame_index = state.frame_index;
                    state.frame_index = (state.frame_index + 1) % Self::LOADING_FRAMES.len();
                    drop(state);

                    let loading_pane = Pane::new(
                        vec![StyledGraphemes::from(Self::LOADING_FRAMES[frame_index])],
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
                Some(event_groups) = rx.recv() => {
                    if let Some(task) = current_task.take() {
                        task.abort();
                        {
                            let mut state = loading_state.lock().await;
                            state.is_loading = false;
                        }
                    }

                    let event_groups = event_groups.clone();
                    let tx_clone = tx.clone();
                    let loading_state = loading_state.clone();

                    let process_task = {
                        let mut this = self.clone();
                        tokio::spawn(async move {
                            {
                                let mut state = loading_state.lock().await;
                                state.is_loading = true;
                            }
                            let result = this.process_event(area, &event_groups).await;
                            {
                                let mut state = loading_state.lock().await;
                                state.is_loading = false;
                            }
                            tx_clone.send(result).await
                        })
                    };

                    current_task = Some(process_task);
                }
                else => {
                    loading_task.abort();
                    break;
                }
            }
        }
    }
}
