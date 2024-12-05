use std::time::Duration;

use crossterm::terminal;
use promkit::{
    crossterm::style::Color,
    style::StyleBuilder,
    switch::ActiveKeySwitcher,
    text_editor::{self},
};
use promkit_async::{component::LoadingComponent, Prompt};

mod lazyutil;
use lazyutil::{component::LazyComponent, keymap};
use tokio::sync::mpsc;

pub struct Lazy {
    keymap: ActiveKeySwitcher<keymap::Handler>,
    text_editor_state: text_editor::State,
}

impl Default for Lazy {
    fn default() -> Self {
        Self {
            keymap: ActiveKeySwitcher::new("default", self::keymap::default),
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

impl Lazy {
    pub async fn run(self) -> anyhow::Result<()> {
        let mut component = LazyComponent::new(self.keymap, self.text_editor_state.clone())?;
        let (event_tx, event_rx) = mpsc::channel(1);
        let (pane_tx, pane_rx) = mpsc::channel(1);
        let terminal_area = terminal::size()?;
        let handle =
            tokio::spawn(async move { component.run(terminal_area, event_rx, pane_tx).await });

        Prompt {}
            .run(vec![event_tx], vec![pane_rx], Duration::from_millis(100))
            .await?;

        handle.abort();
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Lazy::default().run().await
}
