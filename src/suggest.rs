use std::sync::Arc;
use tokio::sync::Mutex;
use serde_json::Value;

struct Inner {
    paths: Vec<String>,
    is_loading: bool,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            is_loading: true,
        }
    }
}

pub struct AsyncSuggest {
    inner: Arc<Mutex<Inner>>,
}

impl AsyncSuggest {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
        }
    }

    pub async fn start_loading(&self, input: &[Value]) {
        let inner = self.inner.clone();
        let input = input.to_vec();
        
        tokio::spawn(async move {
            let paths = jsonz::get_all_paths(&input);
            let mut state = inner.lock().await;
            state.paths = paths;
            state.is_loading = false;
        });
    }

    pub async fn prefix_search(&self, query: &str) -> (Vec<String>, bool) {
        let state = self.inner.lock().await;
        let matches: Vec<String> = state.paths
            .iter()
            .filter(|path| path.starts_with(query))
            .cloned()
            .collect();
        (matches, state.is_loading)
    }
}
