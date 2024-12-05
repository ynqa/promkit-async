use async_trait::async_trait;
use promkit::pane::Pane;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::{sync::mpsc, task::JoinHandle};

struct LoadingState {
    is_loading: bool,
    frame_index: usize,
}

#[async_trait]
pub trait LoadingComponent<T: Clone + Send + Sync + 'static>:
    Clone + Send + Sync + 'static
{
    const LOADING_FRAMES: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    async fn process_event(&mut self, area: (u16, u16), inputs: T) -> Pane;

    async fn rollback_state(&mut self) -> bool;

    async fn run(&mut self, area: (u16, u16), mut rx: mpsc::Receiver<T>, tx: mpsc::Sender<Pane>) {
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
                        vec![promkit::grapheme::StyledGraphemes::from(
                            Self::LOADING_FRAMES[frame_index],
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
                Some(inputs) = rx.recv() => {
                    if let Some(task) = current_task.take() {
                        task.abort();
                        {
                            let mut state = loading_state.lock().await;
                            state.is_loading = false;
                        }
                    }

                    let inputs = inputs.clone();
                    let tx_clone = tx.clone();
                    let loading_state = loading_state.clone();

                    let process_task = {
                        let mut this = self.clone();
                        tokio::spawn(async move {
                            {
                                let mut state = loading_state.lock().await;
                                state.is_loading = true;
                            }
                            let result = this.process_event(area, inputs).await;
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
