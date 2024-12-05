use std::time::Duration;

use promkit::{pane::Pane, switch::ActiveKeySwitcher, text_editor, PaneFactory};

use tokio::{
    sync::mpsc::{Receiver, Sender},
    time::sleep,
};

use promkit_async::{
    component::{Component, LoadingComponent},
    snapshot::AsyncSnapshot,
    EventGroup,
};

use crate::lazyutil::keymap;

#[derive(Clone)]
pub struct LazyComponent {
    keymap: ActiveKeySwitcher<keymap::Handler>,
    state: AsyncSnapshot<text_editor::State>,
}

impl LazyComponent {
    pub fn new(
        keymap: ActiveKeySwitcher<keymap::Handler>,
        state: text_editor::State,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            keymap,
            state: AsyncSnapshot::new(state),
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
        let keymap = self.keymap.get();
        let event_groups = event_groups.clone();
        let area = area;

        self.state
            .current_mut(move |mut state| async move {
                if let Err(e) = keymap(&event_groups, &mut state) {
                    eprintln!("Error processing event: {}", e);
                }
                sleep(Duration::from_secs(5)).await;
                let pane = state.create_pane(area.0, area.1);
                (state, pane)
            })
            .await
    }

    async fn rollback_state(&mut self) -> bool {
        self.state.rollback().await
    }
}
