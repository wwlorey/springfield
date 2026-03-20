# Fixed Issues

## Shell death during long-running sgf AFK loops

**Date:** 2026-03-19

**Symptom:** After ~18 iterations of an AFK loop, Claude Code's Bash tool stops working entirely — all commands (including `echo hello`, `ls`, `date`) return exit code 1 with no output. Subagents are also affected. The iteration's work cannot be committed, tested, or closed.

**Root cause:** Resource exhaustion from leaked file descriptors and orphaned processes accumulating across iterations in `run_afk()`.

1. **Leaked reader threads** (`crates/ralph/src/main.rs`): Each AFK iteration spawned a detached `thread::spawn` to read the `cl` child's stdout via a `BufReader<ChildStdout>`, holding a pipe file descriptor. These threads were never joined — unlike the `ding_watcher` thread in `run_interactive` which was properly joined. Over many iterations, orphaned threads and their pipe FDs accumulated.

2. **No process group cleanup on normal exit**: `kill_process_group()` was only called on shutdown interrupt (Ctrl-C path), not when the `cl` child exited normally. If `cl` spawned any background processes (cargo daemons, language servers, etc.), those became orphans with inherited FDs, further depleting the FD table.

3. **No visibility into resource state**: There was no monitoring of open file descriptors or system limits between iterations, making it impossible to detect the approaching exhaustion before it caused total shell failure.

**Fixes applied to `crates/ralph/src/main.rs`:**

1. **Join reader thread after child exits** — The stdout reader thread is now stored in a `JoinHandle` and explicitly joined after `child.wait()` completes, on both the normal exit path and the shutdown-interrupt path. This ensures the pipe FD is closed and the thread is reclaimed every iteration.

2. **Kill process group on normal exit** — `kill_process_group(child_pid, ...)` is now called before `child.wait()` on the normal (non-interrupt) exit path, reaping any grandchild processes that `cl` may have spawned. This matches the cleanup already done on the interrupt path.

3. **Log resource usage after each iteration** — Added `log_resource_usage()` which logs the current open FD count (via `/dev/fd`) and the `RLIMIT_NOFILE` limit using structured tracing. This runs after every iteration, providing early warning if FDs are accumulating toward the limit.
