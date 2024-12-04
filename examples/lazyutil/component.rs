use std::sync::{Arc, Mutex};

use promkit::{pane::Pane, switch::ActiveKeySwitcher, text_editor};

use tokio::sync::mpsc::{Receiver, Sender};

use promkit_async::{
    component::{Component, LoadingComponent},
    operator::EventGroup,
};

use crate::lazyutil::keymap;

pub struct LazyComponent {
    keymap: ActiveKeySwitcher<keymap::Handler>,
    state: Arc<Mutex<text_editor::State>>,
    lazy_state: Arc<Mutex<text_editor::State>>,
}

impl LazyComponent {
    pub fn new(
        keymap: ActiveKeySwitcher<keymap::Handler>,
        state: text_editor::State,
        lazy_state: text_editor::State,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            keymap,
            state: Arc::new(Mutex::new(state)),
            lazy_state: Arc::new(Mutex::new(lazy_state)),
        })
    }
}

#[async_trait::async_trait]
impl Component for LazyComponent {
    async fn run(&mut self, rx: Receiver<Vec<EventGroup>>, tx: Sender<Pane>) {
        <Self as LoadingComponent>::run(self, rx, tx).await
    }
}

#[async_trait::async_trait]
impl LoadingComponent for LazyComponent {
    async fn process_event(&mut self, _event_group: &EventGroup) -> Pane {
        todo!()
    }
}
