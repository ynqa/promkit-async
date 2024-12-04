use promkit::{pane::Pane, switch::ActiveKeySwitcher, text_editor, PaneFactory};

use tokio::sync::mpsc::{Receiver, Sender};

use promkit_async::{
    component::{Component, LoadingComponent, StateHistory},
    operator::EventGroup,
};

use crate::lazyutil::keymap;

pub struct LazyComponent {
    keymap: ActiveKeySwitcher<keymap::Handler>,
    state: StateHistory<text_editor::State>,
}

impl LazyComponent {
    pub fn new(
        keymap: ActiveKeySwitcher<keymap::Handler>,
        state: text_editor::State,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            keymap,
            state: StateHistory::new(state),
        })
    }
}

#[async_trait::async_trait]
impl Component for LazyComponent {
    async fn run(&mut self, area: (u16, u16), rx: Receiver<Vec<EventGroup>>, tx: Sender<Pane>) {
        <Self as LoadingComponent>::run(self, area, rx, tx).await
    }
}

#[async_trait::async_trait]
impl LoadingComponent for LazyComponent {
    async fn process_event(&mut self, area: (u16, u16), event_groups: &Vec<EventGroup>) -> Pane {
        self.state.with_current_mut(|state| {
            if let Err(e) = self.keymap.get()(event_groups, state) {
                eprintln!("Error processing event: {}", e);
            }
            state.create_pane(area.0, area.1)
        })
    }

    async fn rollback_state(&mut self) -> bool {
        self.state.rollback()
    }
}
