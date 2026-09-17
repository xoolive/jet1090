use crossterm::event::KeyEvent;
use crossterm::execute;
use crossterm::terminal::*;
use futures::{FutureExt, StreamExt};
use ratatui::prelude::*;
use std::io::{self, stdout, Stdout};
use std::panic;
use tokio::sync::mpsc;

/// A type alias for the terminal type used in this application
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// A guard that restores the terminal on drop
/// This ensures the terminal is restored even if the program panics
pub struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = restore();
    }
}

/// Initialize the terminal and set up panic hook for restoration
pub fn init() -> io::Result<Tui> {
    // Install custom panic hook to restore terminal before displaying panic
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = restore();
        original_hook(panic_info);
    }));

    execute!(stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    Terminal::new(CrosstermBackend::new(stdout()))
}

/// Restore the terminal to its original state
pub fn restore() -> io::Result<()> {
    execute!(stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Key(KeyEvent),
    Error,
    Tick(u16),
}

#[derive(Debug)]
pub struct EventHandler {
    _tx: mpsc::UnboundedSender<Event>,
    rx: mpsc::UnboundedReceiver<Event>,
    //task: Option<JoinHandle<()>>,
}

impl EventHandler {
    pub fn new(width: u16) -> Self {
        let tick_rate = std::time::Duration::from_millis(250);

        let (tx, rx) = mpsc::unbounded_channel();
        let _tx = tx.clone();
        let mut width = width;

        let _task = tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            let mut interval = tokio::time::interval(tick_rate);
            loop {
                let delay = interval.tick();
                let crossterm_event = reader.next().fuse();
                tokio::select! {
                  maybe_event = crossterm_event => {
                    match maybe_event {
                      Some(Ok(evt)) => {
                        match evt {
                          crossterm::event::Event::Key(key)
                            if key.kind == crossterm::event::KeyEventKind::Press => {
                              tx.send(Event::Key(key)).unwrap();
                            },
                          crossterm::event::Event::Resize(col,_) => {width = col},
                          crossterm::event::Event::Mouse(event) => {
                            if event.kind == crossterm::event::MouseEventKind::ScrollUp {
                              tx.send(Event::Key(KeyEvent::new(crossterm::event::KeyCode::Char('k'), event.modifiers))).unwrap();
                            }
                            if event.kind == crossterm::event::MouseEventKind::ScrollDown {
                              tx.send(Event::Key(KeyEvent::new(crossterm::event::KeyCode::Char('j'), event.modifiers))).unwrap();
                            }
                          },
                          _ => {},
                        }
                      }
                      Some(Err(_)) => {
                        tx.send(Event::Error).unwrap();
                      }
                      None => {},
                    }
                  },
                  _ = delay => {
                       tx.send(Event::Tick(width)).unwrap_or(());
                  },
                }
            }
        });

        Self {
            _tx,
            rx,
            //task: Some(_task),
        }
    }

    pub async fn next(&mut self) -> Result<Event, io::Error> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| io::Error::other("Unable to get event"))
    }
}
