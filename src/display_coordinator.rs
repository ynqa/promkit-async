use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use futures::Future;
use futures_timer::Delay;
use promkit::{
    crossterm::terminal, grapheme::StyledGraphemes, pane::Pane, style::StyleBuilder,
    terminal::Terminal,
};
use tokio::sync::mpsc::Receiver;

pub struct DisplayCoordinator {
    shared_terminal: Arc<Mutex<Terminal>>,
    version: Arc<AtomicUsize>,
    panes: Arc<Mutex<Vec<Pane>>>,
    delay_duration: Duration,
    frames: Vec<String>,
    actives: Vec<Arc<AtomicBool>>,
    frame_indexes: Vec<Arc<AtomicUsize>>,
}

impl DisplayCoordinator {
    pub fn new(terminal: Terminal, delay_duration: Duration, panes: Vec<Pane>) -> Self {
        let actives = {
            let mut v = Vec::with_capacity(panes.len());
            (0..panes.len()).for_each(|_| v.push(Arc::new(AtomicBool::new(false))));
            v
        };
        let frame_indexes = {
            let mut v = Vec::with_capacity(panes.len());
            (0..panes.len()).for_each(|_| v.push(Arc::new(AtomicUsize::new(0))));
            v
        };
        Self {
            shared_terminal: Arc::new(Mutex::new(terminal)),
            version: Arc::new(AtomicUsize::new(0)),
            panes: Arc::new(Mutex::new(panes)),
            delay_duration,
            frames: ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
                .iter()
                .map(|&s| s.to_string())
                .collect(),
            actives,
            frame_indexes,
        }
    }

    pub fn run(
        &self,
        mut indexed_pane_receiver: Receiver<(usize, usize, Pane)>,
        mut loading_activation_receiver: Receiver<(usize, usize)>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        let global = self.version.clone();
        let shared_panes = Arc::clone(&self.panes);
        let delay_duration = self.delay_duration;
        let actives = self.actives.clone();
        let frames = self.frames.clone();
        let frame_indexes = self.frame_indexes.clone();
        let shared_terminal = Arc::clone(&self.shared_terminal);

        async move {
            loop {
                let delay = Delay::new(delay_duration);
                futures::pin_mut!(delay);

                tokio::select! {
                    maybe_tuple = loading_activation_receiver.recv() => {
                        match maybe_tuple {
                            Some((version, index)) => {
                                if version > global.load(Ordering::SeqCst) {
                                    global.store(version, Ordering::SeqCst);
                                    actives[index].store(true, Ordering::SeqCst);
                                }
                            }
                            None => break,
                        }
                    },
                    maybe_triplet = indexed_pane_receiver.recv() => {
                        match maybe_triplet {
                            Some((version, index, pane)) => {
                                if version >= global.load(Ordering::SeqCst) {
                                    let mut panes = shared_panes.lock().unwrap();
                                    actives[index].store(false, Ordering::SeqCst);
                                    panes[index] = pane;
                                    shared_terminal.lock().unwrap().draw(&panes)?;
                                }
                            }
                            None => break,
                        }
                    },
                    _ = delay => {
                        let tasks: Vec<_> = actives
                            .iter()
                            .enumerate()
                            .filter(|(index, _)| actives[*index].load(Ordering::SeqCst))
                            .map(|(index, active)| {
                                let frames = frames.clone();
                                let shared_panes = Arc::clone(&shared_panes);
                                let frame_indexes = frame_indexes.clone();
                                let shared_terminal = Arc::clone(&shared_terminal);
                                async move {
                                    if active.load(Ordering::SeqCst) {
                                        let frame_index = frame_indexes[index].load(Ordering::SeqCst);
                                        let frame = &frames[frame_index % frames.len()];
                                        let (width, height) = terminal::size()?;
                                        let (matrix, _) = StyledGraphemes::from_str(
                                                frame,
                                                StyleBuilder::new().build(),
                                            ).matrixify(
                                                width as usize,
                                                height as usize,
                                                0,
                                            );
                                        let mut panes = shared_panes.lock().unwrap();
                                        panes[index] = Pane::new(matrix, 0);
                                        shared_terminal.lock().unwrap().draw(&panes)?;
                                        frame_indexes[index].store((frame_index + 1) % frames.len(), Ordering::SeqCst);
                                    }
                                    Ok::<(), anyhow::Error>(())
                                }
                            })
                        .collect();
                        futures::future::join_all(tasks).await;
                    },
                }
            }
            Ok(())
        }
    }
}
