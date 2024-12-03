use std::{
    io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use futures::{stream::StreamExt, Future};
use promkit::{
    crossterm::{
        cursor,
        event::EventStream,
        execute,
        terminal::{disable_raw_mode, enable_raw_mode},
    },
    pane::Pane,
    terminal::Terminal,
};
use tokio::sync::mpsc::Receiver;

pub mod operator;
use operator::{EventGroup, TimeBasedOperator};
pub mod display_coordinator;
use display_coordinator::DisplayCoordinator;

pub trait PaneSyncer: promkit::Finalizer {
    fn init_panes(&self, width: u16, height: u16) -> Vec<Pane>;
    fn sync(
        &mut self,
        version: usize,
        event_buffer: &[EventGroup],
        width: u16,
        height: u16,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
}

pub struct Prompt<T> {
    pub renderer: T,
}

impl<T> Drop for Prompt<T> {
    fn drop(&mut self) {
        execute!(io::stdout(), cursor::MoveToNextLine(1)).ok();
        execute!(io::stdout(), cursor::Show).ok();
        disable_raw_mode().ok();
    }
}

impl<T: PaneSyncer> Prompt<T> {
    pub async fn run(
        &mut self,
        delay: Duration,
        coordinate_delay_duration: Duration,
        mut fin_receiver: Receiver<()>,
        indexed_pane_receiver: Receiver<(usize, usize, Pane)>,
        loading_activation_receiver: Receiver<(usize, usize)>,
    ) -> anyhow::Result<T::Return> {
        enable_raw_mode()?;
        execute!(io::stdout(), cursor::Hide)?;

        let size = crossterm::terminal::size()?;

        let mut operator = TimeBasedOperator::new(delay);
        let (event_sender, event_receiver) = tokio::sync::mpsc::channel(1);
        let (event_buffer_sender, mut event_buffer_receiver) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move { operator.run(event_receiver, event_buffer_sender).await });

        let panes = self.renderer.init_panes(size.0, size.1);

        let mut terminal = Terminal::start_session(&panes)?;
        terminal.draw(&panes)?;

        let coordinator = DisplayCoordinator::new(terminal, coordinate_delay_duration, panes);
        tokio::spawn(async move {
            coordinator
                .run(indexed_pane_receiver, loading_activation_receiver)
                .await
        });

        let mut stream = EventStream::new();

        let version = Arc::new(AtomicUsize::new(1));

        loop {
            tokio::select! {
                maybe_event = stream.next() => {
                    if let Some(Ok(event)) = maybe_event {
                        event_sender.send(event).await?;
                    }
                },
                maybe_event_buffer = event_buffer_receiver.recv() => {
                    if let Some(event_buffer) = maybe_event_buffer {
                        let next = version.fetch_add(1, Ordering::SeqCst);
                        self.renderer.sync(next, &event_buffer, size.0, size.1).await?;
                    }
                },
                maybe_fin = fin_receiver.recv() => {
                    if maybe_fin.is_some() {
                        break;
                    }
                },
            }
        }

        self.renderer.finalize()
    }
}
