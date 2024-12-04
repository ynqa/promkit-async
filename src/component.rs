use std::sync::{Arc, Mutex};
use std::time::Duration;

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

    pub fn with_current<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        let inner = self.inner.lock().unwrap();
        f(&inner.current)
    }

    pub fn with_current_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        let mut inner = self.inner.lock().unwrap();
        f(&mut inner.current)
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
    async fn run(&mut self, mut rx: mpsc::Receiver<Vec<EventGroup>>, tx: mpsc::Sender<Pane>);
}

#[async_trait]
pub trait LoadingComponent: Component {
    const LOADING_FRAMES: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    async fn process_event(&mut self, event_groups: &Vec<EventGroup>) -> Pane;

    async fn rollback_state(&mut self) -> bool;

    async fn run_loading(
        &mut self,
        event_groups: &Vec<EventGroup>,
        cancel_rx: &mut mpsc::Receiver<()>,
    ) -> Option<Pane> {
        let (loading_tx, mut loading_rx): (mpsc::Sender<Pane>, mpsc::Receiver<Pane>) =
            mpsc::channel(1);

        tokio::select! {
            _ = async {
                let mut frame_index = 0;
                let mut interval = tokio::time::interval(Duration::from_millis(100));
                loop {
                    interval.tick().await;
                    let _ = loading_tx.send(Pane::new(
                        vec![StyledGraphemes::from(Self::LOADING_FRAMES[frame_index])],
                        0,
                    )).await;
                    frame_index = (frame_index + 1) % Self::LOADING_FRAMES.len();
                }
            } => unreachable!(),
            result = self.process_event(event_groups) => {
                drop(loading_tx);
                Some(result)
            },
            Some(loading_pane) = loading_rx.recv() => Some(loading_pane),
            _ = cancel_rx.recv() => {
                drop(loading_tx);
                self.rollback_state().await;
                None
            }
        }
    }

    async fn run(&mut self, mut rx: mpsc::Receiver<Vec<EventGroup>>, tx: mpsc::Sender<Pane>) {
        let mut current_cancel_tx: Option<mpsc::Sender<()>> = None;

        while let Some(event_groups) = rx.recv().await {
            if let Some(cancel_tx) = current_cancel_tx.take() {
                let _ = cancel_tx.send(()).await;
            }

            let (new_cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);
            current_cancel_tx = Some(new_cancel_tx);

            loop {
                if let Some(pane) = self.run_loading(&event_groups, &mut cancel_rx).await {
                    if tx.send(pane).await.is_err() {
                        return;
                    }
                    break;
                }
            }
        }
    }
}
