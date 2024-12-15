use std::{fs::File, io::Read, time::Duration};

use crossterm::{event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers}, style::{Attribute, Attributes}, terminal};
use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};
use promkit::{
    crossterm::style::Color, jsonstream::{self, JsonStream}, jsonz::format::RowFormatter, pane::Pane, serde_json::{self, Deserializer}, style::StyleBuilder, switch::ActiveKeySwitcher, text_editor::{self}, PaneFactory
};
use promkit_async::{
    component::{Evaluator, InputProcessor}, Event, Prompt
};
use tokio::sync::mpsc;

pub type TextKeybindings = fn(&[Event], &mut text_editor::State) -> anyhow::Result<()>;
pub fn default(events: &[Event], state: &mut text_editor::State) -> anyhow::Result<()> {
    for event in events {
        match event {
            Event::KeyBuffer(chars) => match state.edit_mode {
                text_editor::Mode::Insert => state.texteditor.insert_chars(&chars),
                text_editor::Mode::Overwrite => state.texteditor.overwrite_chars(&chars),
            },
            Event::HorizontalCursorBuffer(left, right) => {
                state.texteditor.shift(*left, *right);
            }
            Event::Others(e, times) => match e {
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('a'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }) => state.texteditor.move_to_head(),
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('e'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }) => state.texteditor.move_to_tail(),

                // Erase char(s).
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Backspace,
                    modifiers: KeyModifiers::NONE,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }) => {
                    for _ in 0..*times {
                        state.texteditor.erase();
                    }
                }
                crossterm::event::Event::Key(KeyEvent {
                    code: KeyCode::Char('u'),
                    modifiers: KeyModifiers::CONTROL,
                    kind: KeyEventKind::Press,
                    state: KeyEventState::NONE,
                }) => state.texteditor.erase_all(),
                _ => {}
            },
            _ => {}
        }
    }

    Ok(())
}

pub type JsonKeybindings = fn(&[Event], &mut jsonstream::State) -> anyhow::Result<()>;
pub fn movement(events: &[Event], state: &mut jsonstream::State) -> anyhow::Result<()> {
    for event in events {
        match event {
            Event::VerticalCursorBuffer(up, down) => {
                if up > down {
                    for _ in 0..(up-down) {
                        state.stream.up();
                    }
                } else {
                    for _ in 0..(down-up) {
                        state.stream.down();
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}


pub struct Editor {
    keymap: ActiveKeySwitcher<TextKeybindings>,
    state: text_editor::State,
    sync_tx: mpsc::Sender<String>,
}

impl Editor {
    pub fn new(sync_tx: mpsc::Sender<String>) -> anyhow::Result<Self> {
        Ok(Self {
            keymap: ActiveKeySwitcher::new("default", default),
            state: text_editor::State {
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
            sync_tx,
        })
    }
}

impl InputProcessor<Vec<Event>> for Editor {
    fn process_event(&mut self, area: (u16, u16), inputs: Vec<Event>) -> Pane {
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
pub struct Json {
    keymap: ActiveKeySwitcher<JsonKeybindings>,
    state: jsonstream::State,
    json: Vec<serde_json::Value>,
}

impl Json {
    pub fn new(input_stream: Vec<serde_json::Value>) -> anyhow::Result<Self> {
        Ok(Self {
            json: input_stream.clone(),
            keymap: ActiveKeySwitcher::new("default", movement),
            state: jsonstream::State {
                    stream: JsonStream::new(input_stream.iter()),
                    formatter: RowFormatter {
                        curly_brackets_style: StyleBuilder::new()
                            .attrs(Attributes::from(Attribute::Bold))
                            .build(),
                        square_brackets_style: StyleBuilder::new()
                            .attrs(Attributes::from(Attribute::Bold))
                            .build(),
                        key_style: StyleBuilder::new().fgc(Color::Cyan).build(),
                        string_value_style: StyleBuilder::new().fgc(Color::Green).build(),
                        number_value_style: StyleBuilder::new().build(),
                        boolean_value_style: StyleBuilder::new().build(),
                        null_value_style: StyleBuilder::new().fgc(Color::Grey).build(),
                        active_item_attribute: Attribute::Bold,
                        inactive_item_attribute: Attribute::Dim,
                        indent: 2,
                    },
                    lines: Default::default(),
            },
        })
    }
}

#[async_trait::async_trait]
impl Evaluator for Json {
    async fn process_events(&mut self, area: (u16, u16), events: Vec<Event>) -> Pane {
        let keymap = self.keymap.get();
        if let Err(e) = keymap(&events, &mut self.state) {
            eprintln!("Error processing event: {}", e);
        }
        let pane = self.state.create_pane(area.0, area.1);
        pane
    }

    async fn process_query(&mut self, area: (u16, u16), input: String) -> Pane {
        let new = run_jaq(&input, &self.json).ok().unwrap_or_default();
        self.state.stream = JsonStream::new(new.iter());
        let pane = self.state.create_pane(area.0, area.1);
        pane
    }
}

fn run_jaq(
    query: &str,
    json_stream: &Vec<serde_json::Value>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut ret = Vec::<serde_json::Value>::new();

    for input in json_stream {
        let mut ctx = ParseCtx::new(Vec::new());
        ctx.insert_natives(jaq_core::core());
        ctx.insert_defs(jaq_std::std());

        let (f, errs) = jaq_parse::parse(query, jaq_parse::main());
        if !errs.is_empty() {
            let error_message = errs
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(anyhow::anyhow!(error_message));
        }

        let f = ctx.compile(f.unwrap());
        let inputs = RcIter::new(core::iter::empty());
        let mut out = f.run((Ctx::new([], &inputs), Val::from(input.clone())));

        while let Some(Ok(val)) = out.next() {
            ret.push(val.into());
        }
    }

    Ok(ret)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (sync_tx, sync_rx) = mpsc::channel(1);

    let mut ret = String::new();
    File::open("/Users/eufy/workspace/github.com/ynqa/promkit-async/examples/test.json")?.read_to_string(&mut ret)?;
    let deserializer = Deserializer::from_str(&ret).into_iter::<serde_json::Value>();
    let stream = deserializer.collect::<Result<Vec<_>, _>>();

    let mut component1 = Editor::new(sync_tx)?;
    let mut component2 = Json::new(stream?)?;

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
            Duration::from_millis(50),
        )
        .await?;

    handle1.abort();
    handle2.abort();
    Ok(())
}
