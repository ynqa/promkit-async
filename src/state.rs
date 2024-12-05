use std::{future::Future, sync::Arc};
use tokio::sync::Mutex;

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

    pub async fn current_mut<F, Fut, R>(&self, f: F) -> R
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
