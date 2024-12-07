use async_trait::async_trait;
use promkit::pane::Pane;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::{sync::mpsc, task::JoinHandle};

use crate::EventGroup;

#[derive(Clone, PartialEq)]
enum State {
    Idle,
    ProcessQuery,
    ProcessEvents,
}

struct LoadingState {
    frame_index: usize,
    state: State,
}

#[async_trait]
pub trait Evaluator: Clone + Send + Sync + 'static {
    const LOADING_FRAMES: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    async fn process_query(&mut self, area: (u16, u16), query: String) -> Pane;
    async fn process_events(&mut self, area: (u16, u16), events: Vec<EventGroup>) -> Pane;

    async fn run(
        &mut self,
        area: (u16, u16),
        mut query_rx: mpsc::Receiver<String>,
        mut events_rx: mpsc::Receiver<Vec<EventGroup>>,
        tx: mpsc::Sender<Pane>,
    ) {
        let mut current_task: Option<JoinHandle<Result<(), mpsc::error::SendError<Pane>>>> = None;
        let loading_state = Arc::new(Mutex::new(LoadingState {
            frame_index: 0,
            state: State::Idle,
        }));
        let mut event_queue: VecDeque<Vec<EventGroup>> = VecDeque::new();

        let loading_task = {
            let loading_state = loading_state.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_millis(100));
                loop {
                    interval.tick().await;

                    let mut state = loading_state.lock().await;
                    if state.state == State::Idle {
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
                Some(query) = query_rx.recv() => {
                    if let Some(task) = current_task.take() {
                        task.abort();
                    }

                    event_queue.clear();

                    let query = query.clone();
                    let tx_clone = tx.clone();
                    let loading_state = loading_state.clone();

                    let process_task = {
                        let mut this = self.clone();
                        tokio::spawn(async move {
                            {
                                let mut state = loading_state.lock().await;
                                state.state = State::ProcessQuery;
                            }
                            let result = this.process_query(area, query).await;
                            {
                                let mut state = loading_state.lock().await;
                                state.state = State::Idle;
                            }
                            tx_clone.send(result).await
                        })
                    };

                    current_task = Some(process_task);
                }
                Some(events) = events_rx.recv() => {
                    let state = loading_state.lock().await.state.clone();

                    match state {
                        State::ProcessQuery => {
                            continue;
                        }
                        State::ProcessEvents => {
                            event_queue.push_back(events);
                        }
                        State::Idle => {
                            let events = events.clone();
                            let tx_clone = tx.clone();
                            let loading_state = loading_state.clone();

                            let process_task = {
                                let mut this = self.clone();
                                tokio::spawn(async move {
                                    {
                                        let mut state = loading_state.lock().await;
                                        state.state = State::ProcessEvents;
                                    }
                                    let result = this.process_events(area, events).await;
                                    {
                                        let mut state = loading_state.lock().await;
                                        state.state = State::Idle;
                                    }
                                    tx_clone.send(result).await
                                })
                            };

                            if let Some(task) = current_task.take() {
                                task.abort();
                            }
                            current_task = Some(process_task);
                        }
                    }
                }
                else => {
                    loading_task.abort();
                    break;
                }
            }

            if let Some(events) = event_queue.pop_front() {
                if loading_state.lock().await.state == State::Idle {
                    let tx_clone = tx.clone();
                    let loading_state = loading_state.clone();

                    let process_task = {
                        let mut this = self.clone();
                        tokio::spawn(async move {
                            {
                                let mut state = loading_state.lock().await;
                                state.state = State::ProcessEvents;
                            }
                            let result = this.process_events(area, events).await;
                            {
                                let mut state = loading_state.lock().await;
                                state.state = State::Idle;
                            }
                            tx_clone.send(result).await
                        })
                    };

                    if let Some(task) = current_task.take() {
                        task.abort();
                    }
                    current_task = Some(process_task);
                }
            }
        }
    }
}
