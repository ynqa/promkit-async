use std::time::Duration;

use promkit::{pane::Pane, switch::ActiveKeySwitcher, text_editor, PaneFactory};

use tokio::{sync::mpsc, time::sleep};

use promkit_async::{
    component::{LoadingComponent, SyncComponent},
    snapshot::AsyncSnapshot,
    EventGroup,
};

use crate::editorutil::keymap;

pub struct EditorComponent {
    keymap: ActiveKeySwitcher<keymap::Handler>,
    state: text_editor::State,
    sync_tx: mpsc::Sender<String>,
}

impl EditorComponent {
    pub fn new(
        keymap: ActiveKeySwitcher<keymap::Handler>,
        state: text_editor::State,
        sync_tx: mpsc::Sender<String>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            keymap,
            state,
            sync_tx,
        })
    }
}

impl SyncComponent<Vec<EventGroup>> for EditorComponent {
    fn process_event(&mut self, area: (u16, u16), inputs: Vec<EventGroup>) -> Pane {
        let keymap = self.keymap.get();
        if let Err(e) = keymap(&inputs, &mut self.state) {
            eprintln!("Error processing event: {}", e);
        }
        let text = self.state.texteditor.text().to_string();
        let tx = self.sync_tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(text).await;
        });
        self.state.create_pane(area.0, area.1)
    }
}

#[derive(Clone)]
pub struct HeavySyncComponent {
    state: AsyncSnapshot<text_editor::State>,
}

impl HeavySyncComponent {
    pub fn new(state: text_editor::State) -> anyhow::Result<Self> {
        Ok(Self {
            state: AsyncSnapshot::new(state),
        })
    }
}

#[async_trait::async_trait]
impl LoadingComponent<String> for HeavySyncComponent {
    async fn process_event(&mut self, area: (u16, u16), input: String) -> Pane {
        let area = area;

        self.state
            .current_mut(move |mut state| async move {
                state.texteditor.replace(&input.to_uppercase());
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