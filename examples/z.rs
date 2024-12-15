use std::{fs::File, io::Read, time::Duration};

use crossterm::style::{Attribute, Attributes};
use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};
use promkit::{
    crossterm::style::Color,
    jsonstream::{self, JsonStream},
    jsonz::format::RowFormatter,
    pane::Pane,
    serde_json::{self, Deserializer},
    style::StyleBuilder,
    PaneFactory,
};
use promkit_async::{Loader, Visualizer};

#[derive(Clone)]
pub struct Json {
    state: jsonstream::State,
    json: &'static [serde_json::Value],
}

impl Json {
    pub fn new(input_stream: &'static [serde_json::Value]) -> anyhow::Result<Self> {
        Ok(Self {
            json: input_stream,
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
impl Visualizer for Json {
    async fn create_init_pane(&mut self, area: (u16, u16)) -> Pane {
        self.state.create_pane(area.0, area.1)
    }

    async fn create_pane_from(&mut self, area: (u16, u16), input: String) -> Pane {
        let new = run_jaq(&input, self.json).ok().unwrap_or_default();
        self.state.stream = JsonStream::new(new.iter());
        let pane = self.state.create_pane(area.0, area.1);
        pane
    }
}

fn run_jaq(
    query: &str,
    json_stream: &'static [serde_json::Value],
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

struct JsonLoader {}

#[async_trait::async_trait]
impl Loader for JsonLoader {
    async fn load(item: &'static str) -> anyhow::Result<Json> {
        let deserializer = Deserializer::from_str(item).into_iter::<serde_json::Value>();
        let stream = deserializer.collect::<Result<Vec<_>, _>>()?;
        let static_stream = Box::leak(stream.into_boxed_slice());
        Json::new(static_stream)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut content = String::new();
    // File::open("/Users/eufy/workspace/github.com/ynqa/promkit-async/examples/small.json")?
    File::open(format!(
        "{}/examples/large.json",
        std::env::current_dir()?.display()
    ))?
    .read_to_string(&mut content)?;

    promkit_async::run::<JsonLoader>(
        Box::leak(content.into_boxed_str()),
        Duration::from_millis(300),
        Duration::from_millis(600),
        Duration::from_millis(200),
    )
    .await?;

    Ok(())
}
