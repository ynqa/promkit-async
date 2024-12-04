use std::sync::{Arc, Mutex};

use promkit::{pane::Pane, switch::ActiveKeySwitcher, text_editor, PaneFactory};

use futures::Future;
use tokio::sync::mpsc::Sender;

use promkit_async::{component::LoadingComponent, operator::EventGroup};

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

impl LoadingComponent for LazyComponent {
    async fn process_event(&mut self, event_group: &EventGroup) -> Pane {
        todo!()
    }
}
