use std::time::Duration;

use crossterm::terminal;
use promkit::{
    crossterm::style::Color,
    style::StyleBuilder,
    text_editor::{self},
};
use promkit_async::{
    component::{Evaluator, InputProcessor},
    Prompt,
};

mod editorutil;
use editorutil::component::{EditorComponent, HeavySyncComponent};
use tokio::sync::mpsc;

pub struct Editor {
    text_editor_state: text_editor::State,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            text_editor_state: text_editor::State {
                texteditor: Default::default(),
                history: Default::default(),
                prefix: String::from("❯❯ "),
                mask: Default::default(),
                prefix_style: StyleBuilder::new().fgc(Color::DarkGreen).build(),
                active_char_style: StyleBuilder::new().bgc(Color::DarkCyan).build(),
                inactive_char_style: StyleBuilder::new().build(),
                edit_mode: Default::default(),
                word_break_chars: Default::default(),
                lines: Default::default(),
            },
        }
    }
}

impl Editor {
    pub async fn run(self) -> anyhow::Result<()> {
        let (sync_tx, sync_rx) = mpsc::channel(1);

        let mut component1 = EditorComponent::new(self.text_editor_state.clone(), sync_tx)?;
        let mut component2 = HeavySyncComponent::new(self.text_editor_state)?;

        let (event1_tx, event1_rx) = mpsc::channel(1);
        let (event2_tx, event2_rx) = mpsc::channel(1);
        let (pane1_tx, pane1_rx) = mpsc::channel(1);
        let (pane2_tx, pane2_rx) = mpsc::channel(1);

        let terminal_area = terminal::size()?;
        let handle1 =
            tokio::spawn(async move { component1.run(terminal_area, event1_rx, pane1_tx).await });
        let handle2 = tokio::spawn(async move {
            component2
                .run(terminal_area, sync_rx, event2_rx, pane2_tx)
                .await
        });

        Prompt {}
            .run(
                vec![event1_tx, event2_tx],
                vec![pane1_rx, pane2_rx],
                Duration::from_millis(100),
            )
            .await?;

        handle1.abort();
        handle2.abort();
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Editor::default().run().await
}
