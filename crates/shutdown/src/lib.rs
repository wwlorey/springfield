use std::io::{self, Read as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use signal_hook::consts::{SIGINT, SIGTERM};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownStatus {
    Running,
    Pending,
    Shutdown,
}

pub struct ShutdownConfig {
    pub timeout: Duration,
    pub monitor_stdin: bool,
}

impl Default for ShutdownConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(2),
            monitor_stdin: true,
        }
    }
}

struct Inner {
    sigint_count: AtomicU64,
    eof_count: AtomicU64,
    sigterm: Arc<AtomicBool>,
    stop: AtomicBool,
}

pub struct ShutdownController {
    inner: Arc<Inner>,
    timeout: Duration,
    sigint_pending_since: std::cell::Cell<Option<Instant>>,
    eof_pending_since: std::cell::Cell<Option<Instant>>,
    last_seen_sigint: std::cell::Cell<u64>,
    last_seen_eof: std::cell::Cell<u64>,
    confirmed_shutdown: std::cell::Cell<bool>,
}

impl ShutdownController {
    pub fn new(config: ShutdownConfig) -> io::Result<Self> {
        let inner = Arc::new(Inner {
            sigint_count: AtomicU64::new(0),
            eof_count: AtomicU64::new(0),
            sigterm: Arc::new(AtomicBool::new(false)),
            stop: AtomicBool::new(false),
        });

        let inner_sigint = Arc::clone(&inner);
        unsafe {
            signal_hook::low_level::register(SIGINT, move || {
                inner_sigint.sigint_count.fetch_add(1, Ordering::SeqCst);
            })?;
        }

        signal_hook::flag::register(SIGTERM, Arc::clone(&inner.sigterm))?;

        if config.monitor_stdin {
            let inner_stdin = Arc::clone(&inner);
            thread::Builder::new()
                .name("shutdown-stdin".into())
                .spawn(move || {
                    let stdin = io::stdin();
                    let mut handle = stdin.lock();
                    let mut buf = [0u8; 256];
                    while !inner_stdin.stop.load(Ordering::Relaxed) {
                        match handle.read(&mut buf) {
                            Ok(0) => {
                                inner_stdin.eof_count.fetch_add(1, Ordering::SeqCst);
                            }
                            Ok(_) => {}
                            Err(_) => break,
                        }
                    }
                })?;
        }

        Ok(Self {
            inner,
            timeout: config.timeout,
            sigint_pending_since: std::cell::Cell::new(None),
            eof_pending_since: std::cell::Cell::new(None),
            last_seen_sigint: std::cell::Cell::new(0),
            last_seen_eof: std::cell::Cell::new(0),
            confirmed_shutdown: std::cell::Cell::new(false),
        })
    }

    pub fn poll(&self) -> ShutdownStatus {
        if self.confirmed_shutdown.get() {
            return ShutdownStatus::Shutdown;
        }

        if self.inner.sigterm.load(Ordering::SeqCst) {
            self.confirmed_shutdown.set(true);
            return ShutdownStatus::Shutdown;
        }

        let now = Instant::now();
        let sigint = self.inner.sigint_count.load(Ordering::SeqCst);
        let eof = self.inner.eof_count.load(Ordering::SeqCst);

        let prev_sigint = self.last_seen_sigint.get();
        let prev_eof = self.last_seen_eof.get();

        let new_sigint = sigint > prev_sigint;
        let new_eof = eof > prev_eof;

        if new_sigint {
            self.last_seen_sigint.set(sigint);

            if let Some(since) = self.sigint_pending_since.get()
                && now.duration_since(since) <= self.timeout
            {
                self.confirmed_shutdown.set(true);
                return ShutdownStatus::Shutdown;
            }

            self.eof_pending_since.set(None);
            self.sigint_pending_since.set(Some(now));
            eprintln!("Press Ctrl-C again to exit");
            return ShutdownStatus::Pending;
        }

        if new_eof {
            self.last_seen_eof.set(eof);

            if let Some(since) = self.eof_pending_since.get()
                && now.duration_since(since) <= self.timeout
            {
                self.confirmed_shutdown.set(true);
                return ShutdownStatus::Shutdown;
            }

            self.sigint_pending_since.set(None);
            self.eof_pending_since.set(Some(now));
            eprintln!("Press Ctrl-D again to exit");
            return ShutdownStatus::Pending;
        }

        if let Some(since) = self.sigint_pending_since.get() {
            if now.duration_since(since) > self.timeout {
                self.sigint_pending_since.set(None);
            } else {
                return ShutdownStatus::Pending;
            }
        }

        if let Some(since) = self.eof_pending_since.get() {
            if now.duration_since(since) > self.timeout {
                self.eof_pending_since.set(None);
            } else {
                return ShutdownStatus::Pending;
            }
        }

        ShutdownStatus::Running
    }
}

impl Drop for ShutdownController {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::sys::signal::{self, Signal};
    use nix::unistd::Pid;

    fn controller_no_stdin() -> ShutdownController {
        ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            ..Default::default()
        })
        .unwrap()
    }

    fn controller_with_timeout(timeout: Duration) -> ShutdownController {
        ShutdownController::new(ShutdownConfig {
            monitor_stdin: false,
            timeout,
        })
        .unwrap()
    }

    #[test]
    fn default_config() {
        let config = ShutdownConfig::default();
        assert_eq!(config.timeout, Duration::from_secs(2));
        assert!(config.monitor_stdin);
    }

    #[test]
    fn poll_returns_running_initially() {
        let ctrl = controller_no_stdin();
        assert_eq!(ctrl.poll(), ShutdownStatus::Running);
    }

    #[test]
    fn sigterm_immediate_shutdown() {
        let ctrl = controller_no_stdin();
        signal::kill(Pid::this(), Signal::SIGTERM).unwrap();
        thread::sleep(Duration::from_millis(10));
        assert_eq!(ctrl.poll(), ShutdownStatus::Shutdown);
    }

    #[test]
    fn single_sigint_returns_pending() {
        let ctrl = controller_no_stdin();
        signal::kill(Pid::this(), Signal::SIGINT).unwrap();
        thread::sleep(Duration::from_millis(10));
        assert_eq!(ctrl.poll(), ShutdownStatus::Pending);
    }

    #[test]
    fn double_sigint_returns_shutdown() {
        let ctrl = controller_no_stdin();
        signal::kill(Pid::this(), Signal::SIGINT).unwrap();
        thread::sleep(Duration::from_millis(10));
        assert_eq!(ctrl.poll(), ShutdownStatus::Pending);

        signal::kill(Pid::this(), Signal::SIGINT).unwrap();
        thread::sleep(Duration::from_millis(10));
        assert_eq!(ctrl.poll(), ShutdownStatus::Shutdown);
    }

    #[test]
    fn sigint_resets_after_timeout() {
        let ctrl = controller_with_timeout(Duration::from_millis(100));
        signal::kill(Pid::this(), Signal::SIGINT).unwrap();
        thread::sleep(Duration::from_millis(10));
        assert_eq!(ctrl.poll(), ShutdownStatus::Pending);

        thread::sleep(Duration::from_millis(150));
        assert_eq!(ctrl.poll(), ShutdownStatus::Running);
    }
}
