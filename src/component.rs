use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use promkit::{grapheme::StyledGraphemes, pane::Pane};
use tokio::sync::mpsc;

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

    pub fn update(&self, new_state: T) {
        let mut inner = self.inner.lock().unwrap();
        inner.previous = Some(inner.current.clone());
        inner.current = new_state;
    }

    pub fn rollback(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
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
            let inner = self.inner.lock().unwrap();
            inner.current.clone()
        };

        let (new_state, result) = f(current).await;
        self.update(new_state);
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

#[async_trait]
pub trait LoadingComponent: Component {
    const LOADING_FRAMES: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    async fn process_event(&mut self, area: (u16, u16), event_groups: &Vec<EventGroup>) -> Pane;

    async fn rollback_state(&mut self) -> bool;

    async fn run(
        &mut self,
        area: (u16, u16),
        mut rx: mpsc::Receiver<Vec<EventGroup>>,
        tx: mpsc::Sender<Pane>,
    ) {
        loop {
            if let Some(event_groups) = rx.recv().await {
                let event_groups = event_groups.clone();

                let loading_task = tokio::spawn({
                    let tx = tx.clone();
                    async move {
                        let mut frame_index = 0;
                        let mut interval = tokio::time::interval(Duration::from_millis(100));
                        loop {
                            let loading_pane = Pane::new(
                                vec![StyledGraphemes::from(Self::LOADING_FRAMES[frame_index])],
                                0,
                            );
                            if tx.send(loading_pane).await.is_err() {
                                break;
                            }
                            frame_index = (frame_index + 1) % Self::LOADING_FRAMES.len();
                            interval.tick().await;
                        }
                    }
                });

                let result = self.process_event(area, &event_groups).await;

                loading_task.abort();

                if tx.send(result).await.is_err() {
                    return;
                }
            }
        }
    }
}
