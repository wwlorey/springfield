use std::io::{self, Read as _};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use signal_hook::SigId;
use signal_hook::consts::{SIGINT, SIGTERM};

const CHILD_GUARD_KILL_TIMEOUT: Duration = Duration::from_millis(200);

pub struct ChildGuard {
    child: Option<Child>,
    pid: u32,
}

impl ChildGuard {
    pub fn spawn(cmd: &mut Command) -> io::Result<Self> {
        unsafe {
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }
        let child = cmd.spawn()?;
        let pid = child.id();
        Ok(Self {
            child: Some(child),
            pid,
        })
    }

    pub fn new(child: Child) -> Self {
        let pid = child.id();
        Self {
            child: Some(child),
            pid,
        }
    }

    pub fn id(&self) -> u32 {
        self.pid
    }

    pub fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("child already consumed")
    }

    pub fn wait_with_output(mut self) -> io::Result<Output> {
        let child = self.child.take().expect("child already consumed");
        child.wait_with_output()
    }

    pub fn wait_with_output_timeout(mut self, timeout: Duration) -> io::Result<Output> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.try_wait()? {
                Some(status) => {
                    #[allow(clippy::zombie_processes)]
                    let mut child = self.child.take().expect("child already consumed");
                    let mut stdout = Vec::new();
                    let mut stderr = Vec::new();
                    if let Some(mut out) = child.stdout.take() {
                        let _ = out.read_to_end(&mut stdout);
                    }
                    if let Some(mut err) = child.stderr.take() {
                        let _ = err.read_to_end(&mut stderr);
                    }
                    return Ok(Output {
                        status,
                        stdout,
                        stderr,
                    });
                }
                None if Instant::now() >= deadline => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!(
                            "child process {} did not exit within {:?}",
                            self.pid, timeout
                        ),
                    ));
                }
                None => {
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child_mut().try_wait()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };

        if !kill_process_group(self.pid, CHILD_GUARD_KILL_TIMEOUT) {
            let _ = child.kill();
        }

        let _ = child.wait();
    }
}

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
    signal_ids: Vec<SigId>,
}

impl ShutdownController {
    pub fn new(config: ShutdownConfig) -> io::Result<Self> {
        let inner = Arc::new(Inner {
            sigint_count: AtomicU64::new(0),
            eof_count: AtomicU64::new(0),
            sigterm: Arc::new(AtomicBool::new(false)),
            stop: AtomicBool::new(false),
        });

        let mut signal_ids = Vec::new();

        let inner_sigint = Arc::clone(&inner);
        let sigint_id = unsafe {
            signal_hook::low_level::register(SIGINT, move || {
                inner_sigint.sigint_count.fetch_add(1, Ordering::SeqCst);
            })?
        };
        signal_ids.push(sigint_id);

        let sigterm_id = signal_hook::flag::register(SIGTERM, Arc::clone(&inner.sigterm))?;
        signal_ids.push(sigterm_id);

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
            signal_ids,
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

pub fn kill_process_group(pid: u32, timeout: Duration) -> bool {
    let neg_pid = -(pid as i32);

    if unsafe { libc::kill(neg_pid, libc::SIGTERM) } != 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            return false;
        }
    }

    let start = Instant::now();
    while start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(100));
        if unsafe { libc::kill(pid as i32, 0) } != 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ESRCH) {
                return true;
            }
        }
    }

    unsafe { libc::kill(neg_pid, libc::SIGKILL) };
    true
}

impl Drop for ShutdownController {
    fn drop(&mut self) {
        for id in &self.signal_ids {
            signal_hook::low_level::unregister(*id);
        }
        self.inner.stop.store(true, Ordering::Relaxed);
    }
}

pub struct ProcessSemaphore {
    mutex: Mutex<usize>,
    condvar: Condvar,
    max: usize,
}

impl ProcessSemaphore {
    pub fn new(max: usize) -> Self {
        Self {
            mutex: Mutex::new(0),
            condvar: Condvar::new(),
            max,
        }
    }

    pub fn from_env(var: &str, default: usize) -> Self {
        let max = std::env::var(var)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default);
        Self::new(max)
    }

    pub fn max(&self) -> usize {
        self.max
    }

    pub fn acquire(&self) -> ProcessSemaphoreGuard<'_> {
        let mut count = self.mutex.lock().unwrap();
        while *count >= self.max {
            count = self.condvar.wait(count).unwrap();
        }
        *count += 1;
        ProcessSemaphoreGuard { sem: self }
    }

    pub fn acquire_timeout(&self, timeout: Duration) -> Option<ProcessSemaphoreGuard<'_>> {
        let deadline = Instant::now() + timeout;
        let mut count = self.mutex.lock().unwrap();
        while *count >= self.max {
            let remaining = deadline.checked_duration_since(Instant::now())?;
            let (new_count, wait_result) = self.condvar.wait_timeout(count, remaining).unwrap();
            count = new_count;
            if wait_result.timed_out() && *count >= self.max {
                return None;
            }
        }
        *count += 1;
        Some(ProcessSemaphoreGuard { sem: self })
    }
}

pub struct ProcessSemaphoreGuard<'a> {
    sem: &'a ProcessSemaphore,
}

impl Drop for ProcessSemaphoreGuard<'_> {
    fn drop(&mut self) {
        let mut count = self.sem.mutex.lock().unwrap();
        *count -= 1;
        self.sem.condvar.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::sys::signal::{self, Signal};
    use nix::unistd::Pid;
    use serial_test::serial;

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
    #[serial]
    fn sigterm_immediate_shutdown() {
        let ctrl = controller_no_stdin();
        signal::kill(Pid::this(), Signal::SIGTERM).unwrap();
        thread::sleep(Duration::from_millis(10));
        assert_eq!(ctrl.poll(), ShutdownStatus::Shutdown);
    }

    #[test]
    #[serial]
    fn single_sigint_returns_pending() {
        let ctrl = controller_no_stdin();
        signal::kill(Pid::this(), Signal::SIGINT).unwrap();
        thread::sleep(Duration::from_millis(10));
        assert_eq!(ctrl.poll(), ShutdownStatus::Pending);
    }

    #[test]
    #[serial]
    fn double_sigint_returns_shutdown() {
        let ctrl = controller_no_stdin();
        signal::kill(Pid::this(), Signal::SIGINT).unwrap();
        thread::sleep(Duration::from_millis(10));
        assert_eq!(ctrl.poll(), ShutdownStatus::Pending);

        signal::kill(Pid::this(), Signal::SIGINT).unwrap();
        thread::sleep(Duration::from_millis(10));
        assert_eq!(ctrl.poll(), ShutdownStatus::Shutdown);
    }

    use std::os::unix::process::CommandExt;
    use std::process::Command;

    fn spawn_in_new_session(args: &[&str]) -> std::process::Child {
        let mut cmd = Command::new(args[0]);
        if args.len() > 1 {
            cmd.args(&args[1..]);
        }
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    }

    fn wait_for_pid_dead(pid: u32, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if unsafe { libc::kill(pid as i32, 0) } != 0 {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn kill_pg_sends_sigterm_to_group() {
        use std::os::unix::process::ExitStatusExt;

        let mut child = spawn_in_new_session(&["sleep", "60"]);
        let pid = child.id();
        thread::sleep(Duration::from_millis(100));

        let result = kill_process_group(pid, Duration::from_secs(5));
        assert!(result);

        let status = child.wait().unwrap();
        assert!(!status.success());
        assert_eq!(
            status.signal(),
            Some(libc::SIGTERM),
            "child should be killed by SIGTERM, not SIGKILL"
        );
    }

    #[test]
    fn kill_pg_escalates_to_sigkill() {
        let mut child = spawn_in_new_session(&["sh", "-c", "trap '' TERM; sleep 60"]);
        let pid = child.id();
        thread::sleep(Duration::from_millis(100));

        let result = kill_process_group(pid, Duration::from_millis(500));
        assert!(result);

        let status = child.wait().unwrap();
        assert!(!status.success());
    }

    #[test]
    fn kill_pg_already_dead() {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        thread::sleep(Duration::from_millis(50));

        let result = kill_process_group(pid, Duration::from_secs(1));
        assert!(!result);
    }

    #[test]
    fn kill_pg_kills_descendants() {
        let mut child = spawn_in_new_session(&["sh", "-c", "sleep 60 & sleep 60 & wait"]);
        let pid = child.id();
        thread::sleep(Duration::from_millis(200));

        let result = kill_process_group(pid, Duration::from_secs(5));
        assert!(result);

        child.wait().unwrap();

        thread::sleep(Duration::from_millis(200));
        assert!(wait_for_pid_dead(pid, Duration::from_secs(2)));
    }

    #[test]
    #[serial]
    fn sigint_resets_after_timeout() {
        let ctrl = controller_with_timeout(Duration::from_millis(100));
        signal::kill(Pid::this(), Signal::SIGINT).unwrap();
        thread::sleep(Duration::from_millis(10));
        assert_eq!(ctrl.poll(), ShutdownStatus::Pending);

        thread::sleep(Duration::from_millis(150));
        assert_eq!(ctrl.poll(), ShutdownStatus::Running);
    }

    fn spawn_guard(args: &[&str]) -> ChildGuard {
        let mut cmd = Command::new(args[0]);
        if args.len() > 1 {
            cmd.args(&args[1..]);
        }
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        ChildGuard::spawn(&mut cmd).unwrap()
    }

    #[test]
    fn drop_kills_running_process() {
        let guard = spawn_guard(&["sleep", "60"]);
        let pid = guard.id();
        thread::sleep(Duration::from_millis(50));
        drop(guard);
        assert!(wait_for_pid_dead(pid, Duration::from_secs(5)));
    }

    #[test]
    fn drop_kills_descendants() {
        let guard = spawn_guard(&["sh", "-c", "sleep 60 & sleep 60 & wait"]);
        let pid = guard.id();
        thread::sleep(Duration::from_millis(200));
        drop(guard);
        assert!(wait_for_pid_dead(pid, Duration::from_secs(5)));
    }

    #[test]
    fn drop_during_panic_cleans_up() {
        let pid;
        {
            let guard = spawn_guard(&["sleep", "60"]);
            pid = guard.id();
            thread::sleep(Duration::from_millis(50));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = guard;
                panic!("intentional panic");
            }));
            assert!(result.is_err());
        }
        assert!(wait_for_pid_dead(pid, Duration::from_secs(5)));
    }

    #[test]
    fn wait_with_output_consumes_child() {
        let guard = spawn_guard(&["true"]);
        let pid = guard.id();
        let output = guard.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(wait_for_pid_dead(pid, Duration::from_secs(2)));
    }

    #[test]
    fn wait_with_output_timeout_succeeds() {
        let mut cmd = Command::new("echo");
        cmd.arg("hello")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let guard = ChildGuard::spawn(&mut cmd).unwrap();
        let output = guard
            .wait_with_output_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
    }

    #[test]
    fn wait_with_output_timeout_kills_on_expire() {
        let guard = spawn_guard(&["sleep", "60"]);
        let pid = guard.id();
        let result = guard.wait_with_output_timeout(Duration::from_millis(200));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(wait_for_pid_dead(pid, Duration::from_secs(5)));
    }

    #[test]
    fn already_exited_no_error() {
        let guard = spawn_guard(&["true"]);
        let pid = guard.id();
        thread::sleep(Duration::from_millis(200));
        drop(guard);
        assert!(wait_for_pid_dead(pid, Duration::from_secs(2)));
    }

    #[test]
    fn no_zombie_after_drop() {
        let guard = spawn_guard(&["true"]);
        let pid = guard.id();
        thread::sleep(Duration::from_millis(200));
        drop(guard);
        thread::sleep(Duration::from_millis(100));
        let ret = unsafe { libc::waitpid(pid as i32, std::ptr::null_mut(), libc::WNOHANG) };
        assert!(ret == 0 || ret == -1, "no zombie: waitpid returned {ret}");
        if ret == -1 {
            assert_eq!(
                io::Error::last_os_error().raw_os_error(),
                Some(libc::ECHILD)
            );
        }
    }

    #[test]
    fn fallback_kills_non_group_leader() {
        let child = Command::new("sleep")
            .arg("60")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let guard = ChildGuard::new(child);
        thread::sleep(Duration::from_millis(50));
        drop(guard);
        assert!(wait_for_pid_dead(pid, Duration::from_secs(5)));
    }

    #[test]
    fn concurrent_guards_all_cleanup() {
        let mut pids = Vec::new();
        let mut guards = Vec::new();
        for _ in 0..10 {
            let g = spawn_guard(&["sleep", "60"]);
            pids.push(g.id());
            guards.push(g);
        }
        thread::sleep(Duration::from_millis(100));
        drop(guards);
        for pid in &pids {
            assert!(
                wait_for_pid_dead(*pid, Duration::from_secs(5)),
                "pid {pid} still alive"
            );
        }
    }

    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #![proptest_config(proptest::prelude::ProptestConfig::with_cases(16))]
            #[test]
            #[ignore]
            fn no_process_leak(n in 1usize..=12) {
                let mut pids = Vec::new();
                let mut guards = Vec::new();
                for _ in 0..n {
                    let g = spawn_guard(&["sleep", "60"]);
                    pids.push(g.id());
                    guards.push(g);
                }
                thread::sleep(Duration::from_millis(100));
                drop(guards);
                for pid in &pids {
                    prop_assert!(
                        wait_for_pid_dead(*pid, Duration::from_secs(5)),
                        "pid {} still alive", pid
                    );
                }
            }
        }
    }

    mod semaphore_tests {
        use super::*;

        #[test]
        fn acquire_up_to_max() {
            let sem = ProcessSemaphore::new(4);
            let _g1 = sem.acquire();
            let _g2 = sem.acquire();
            let _g3 = sem.acquire();
            let _g4 = sem.acquire();
        }

        #[test]
        fn acquire_blocks_at_max() {
            use std::sync::Arc;
            use std::sync::atomic::{AtomicBool, Ordering};

            let sem = Arc::new(ProcessSemaphore::new(1));
            let _g = sem.acquire();

            let blocked = Arc::new(AtomicBool::new(true));
            let blocked_clone = Arc::clone(&blocked);
            let sem_clone = Arc::clone(&sem);

            let handle = thread::spawn(move || {
                let _g2 = sem_clone.acquire();
                blocked_clone.store(false, Ordering::SeqCst);
            });

            thread::sleep(Duration::from_millis(100));
            assert!(blocked.load(Ordering::SeqCst), "thread should be blocked");

            drop(_g);
            handle.join().unwrap();
            assert!(
                !blocked.load(Ordering::SeqCst),
                "thread should have unblocked"
            );
        }

        #[test]
        fn guard_releases_on_drop() {
            let sem = ProcessSemaphore::new(1);
            let g = sem.acquire();
            drop(g);
            let _g2 = sem.acquire();
        }

        #[test]
        fn acquire_timeout_returns_none_on_expire() {
            let sem = ProcessSemaphore::new(1);
            let _g = sem.acquire();
            let result = sem.acquire_timeout(Duration::from_millis(200));
            assert!(result.is_none());
        }

        #[test]
        fn acquire_timeout_succeeds_when_available() {
            let sem = ProcessSemaphore::new(2);
            let _g1 = sem.acquire();
            let g2 = sem.acquire_timeout(Duration::from_millis(200));
            assert!(g2.is_some());
        }

        #[test]
        fn from_env_reads_variable() {
            let key = "SGF_TEST_SEM_READ_VAR_TEST";
            unsafe { std::env::set_var(key, "3") };
            let sem = ProcessSemaphore::from_env(key, 10);
            assert_eq!(sem.max(), 3);
            unsafe { std::env::remove_var(key) };
        }

        #[test]
        fn from_env_uses_default() {
            let key = "SGF_TEST_SEM_NONEXISTENT_VAR";
            unsafe { std::env::remove_var(key) };
            let sem = ProcessSemaphore::from_env(key, 5);
            assert_eq!(sem.max(), 5);
        }

        mod prop {
            use super::*;
            use proptest::prelude::*;
            use std::sync::Arc;
            use std::sync::atomic::{AtomicUsize, Ordering};

            proptest! {
                #![proptest_config(ProptestConfig::with_cases(32))]
                #[test]
                fn active_permits_never_exceed_max(max in 2usize..=8, n_threads in 2usize..=16) {
                    let sem = Arc::new(ProcessSemaphore::new(max));
                    let active = Arc::new(AtomicUsize::new(0));
                    let violated = Arc::new(AtomicBool::new(false));

                    let handles: Vec<_> = (0..n_threads).map(|_| {
                        let sem = Arc::clone(&sem);
                        let active = Arc::clone(&active);
                        let violated = Arc::clone(&violated);
                        thread::spawn(move || {
                            let _g = sem.acquire();
                            let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                            if current > max {
                                violated.store(true, Ordering::SeqCst);
                            }
                            thread::sleep(Duration::from_millis(5));
                            active.fetch_sub(1, Ordering::SeqCst);
                        })
                    }).collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                    prop_assert!(!violated.load(Ordering::SeqCst), "active permits exceeded max");
                }
            }
        }
    }
}
