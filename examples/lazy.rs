use std::time::Duration;

use promkit::{
    crossterm::style::Color,
    style::StyleBuilder,
    switch::ActiveKeySwitcher,
    text_editor::{self},
};
use promkit_async::Prompt;

mod lazyutil;
use lazyutil::{component, keymap};

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
        let component = component::LazyComponent::new(self.keymap, self.text_editor_state.clone())?;

        Prompt {}
            .run(vec![Box::new(component)], Duration::from_millis(10))
            .await
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Lazy::default().run().await
}
