use std::{io, sync::Arc, time::Duration};

use crossterm::{
    self, cursor,
    event::{Event, EventStream},
    execute,
    terminal::{self, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use futures_timer::Delay;
use promkit::{pane::Pane, style::StyleBuilder, terminal::Terminal, text, PaneFactory};
use tokio::{
    sync::{mpsc, Mutex, RwLock},
    task::JoinHandle,
};

#[derive(Debug)]
pub enum PaneIndex {
    Editor = 0,
    Hint = 1,
    Suggst = 2,
    Data = 3,
}

const PANESIZE: usize = PaneIndex::Data as usize + 1;

mod editor;
pub use editor::Editor;
mod wizard;
pub use wizard::{Context, Loader, Spinner, Visualizer, Wizard};

pub enum Action {
    None,
    MoveCursor,
    ChangeText,
    Quit,
}

fn spawn_debouncer<T: Send + 'static>(
    mut debounce_rx: mpsc::Receiver<T>,
    last_tx: mpsc::Sender<T>,
    duration: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_query = None;
        loop {
            let delay = Delay::new(duration);
            futures::pin_mut!(delay);

            tokio::select! {
                maybe_query = debounce_rx.recv() => {
                    if let Some(query) = maybe_query {
                        last_query = Some(query);
                    } else {
                        break;
                    }
                },
                _ = delay => {
                    if let Some(text) = last_query.take() {
                        let _ = last_tx.send(text).await;
                    }
                },
            }
        }
    })
}

pub async fn run<L: Loader>(
    item: &'static str,
    spin_duration: Duration,
    query_debounce_duration: Duration,
    resize_debounce_duration: Duration,
) -> anyhow::Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), cursor::Hide)?;

    let size = terminal::size()?;

    let editor = Editor::default();
    let hint = text::State {
        text: String::from("welcome"),
        style: StyleBuilder::new()
            .fgc(crossterm::style::Color::Grey)
            .build(),
    };

    let mut init_terminal = Terminal {
        position: cursor::position()?,
    };
    let init_panes = [
        editor.create_pane(size.0, size.1),
        hint.create_pane(size.0, size.1),
        Pane::new(vec![], 0),
        Pane::new(vec![], 0),
    ];
    init_terminal.draw(&init_panes)?;

    let ctx = Arc::new(Mutex::new(Context::new(size)));

    let shared_terminal = Arc::new(Mutex::new(init_terminal));
    let shared_panes: Arc<Mutex<[Pane; PANESIZE]>> = Arc::new(Mutex::new(init_panes));

    let (last_query_tx, mut last_query_rx) = mpsc::channel(1);
    let (debounce_query_tx, debounce_query_rx) = mpsc::channel(1);
    let query_debouncer =
        spawn_debouncer(debounce_query_rx, last_query_tx, query_debounce_duration);

    let (last_resize_tx, mut last_resize_rx) = mpsc::channel::<(u16, u16)>(1);
    let (debounce_resize_tx, debounce_resize_rx) = mpsc::channel(1);
    let resize_debouncer =
        spawn_debouncer(debounce_resize_rx, last_resize_tx, resize_debounce_duration);

    let spinner = Spinner::new(ctx.clone());
    let (spinner_panes, spinner_terminal) = (shared_panes.clone(), shared_terminal.clone());
    let spinning = spinner.spawn_spin_task(spinner_panes, spinner_terminal, spin_duration);

    let shared_editor = Arc::new(RwLock::new(editor));

    let main_task: JoinHandle<anyhow::Result<()>> = {
        let mut stream = EventStream::new();
        let shared_terminal = shared_terminal.clone();
        let shared_panes = shared_panes.clone();
        let shared_editor = shared_editor.clone();
        tokio::spawn(async move {
            'main: loop {
                tokio::select! {
                    Some(Ok(event)) = stream.next() => {
                        if let Event::Resize(width, height) = event {
                            debounce_resize_tx.send((width, height)).await?;
                        } else {
                            let size = terminal::size()?;
                            let pane = {
                                let mut editor = shared_editor.write().await;
                                match editor.evaluate(&event)? {
                                    Action::None => {
                                        continue 'main;
                                    },
                                    Action::Quit => {
                                        break 'main;
                                    }
                                    Action::ChangeText => {
                                        debounce_query_tx.send(editor.text()).await?;
                                    }
                                    Action::MoveCursor => (),
                                }
                                editor.create_pane(size.0, size.1)
                            };
                            {
                                let mut panes = shared_panes.lock().await;
                                let mut terminal = shared_terminal.lock().await;
                                panes[PaneIndex::Editor as usize] = pane;
                                terminal.draw(&*panes)?;
                            }
                        }
                    },
                    else => {
                        break 'main;
                    }
                }
            }
            Ok(())
        })
    };

    let processor_task: JoinHandle<anyhow::Result<()>> = {
        let wiz = Wizard::new(ctx.clone());
        let shared_terminal = shared_terminal.clone();
        let shared_panes = shared_panes.clone();
        let shared_editor = shared_editor.clone();
        let mut visualizer = wiz.loading::<L>(item).await?;
        let pane = visualizer.create_init_pane(size).await;
        {
            let mut panes = shared_panes.lock().await;
            panes[PaneIndex::Data as usize] = pane;
            let mut terminal = shared_terminal.lock().await;
            terminal.draw(&*panes)?;
        }
        let shared_visualizer = Arc::new(Mutex::new(visualizer));
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(query) = last_query_rx.recv() => {
                        wiz.evaluate(shared_visualizer.clone(), query, shared_terminal.clone(), shared_panes.clone()).await;
                    }
                    Some(area) = last_resize_rx.recv() => {
                        let pane = {
                            let editor = shared_editor.read().await;
                            editor.create_pane(area.0, area.1)
                        };
                        {
                            let mut panes = shared_panes.lock().await;
                            let mut terminal = shared_terminal.lock().await;
                            panes[PaneIndex::Editor as usize] = pane;
                            terminal.draw(&*panes)?;
                        }
                        let text = {
                            let editor = shared_editor.read().await;
                            editor.text()
                        };
                        wiz.write_on_resize(shared_visualizer.clone(), area, text, shared_terminal.clone(), shared_panes.clone()).await;
                    }
                    else => {
                        break
                    }
                }
            }
            Ok(())
        })
    };

    main_task.await??;

    spinning.abort();
    query_debouncer.abort();
    resize_debouncer.abort();
    processor_task.abort();

    execute!(io::stdout(), cursor::Show)?;
    disable_raw_mode()?;

    Ok(())
}
