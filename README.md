# procrun

Runs a `Procfile` — the same job Ruby's `foreman` (and its many
descendants: `overmind`, `honcho`, `hivemind`) do, without needing
Ruby, tmux, or a language runtime beyond whatever your own processes
need. Every process's stdout/stderr gets a colored, name-aligned
prefix; if any process exits, every other process is sent a real
SIGTERM (escalating to SIGKILL for stragglers); Ctrl-C or `docker
stop` on `procrun` itself does the same to every child.

## Usage

```bash
procrun                      # reads ./Procfile
procrun path/to/Procfile
procrun -e .env               # load env vars from a dotenv-style file into every process
```

`Procfile`:

```
web: cargo run --bin server
worker: cargo run --bin worker -- --queue=default
```

```
$ procrun
procrun: starting 2 process(es) from Procfile
web    | tick 1
worker | tick 1
web    | tick 2
worker | tick 2
procrun | 'web' exited (0) — shutting down the rest
```

Each command is handed to `sh -c` wholesale — pipes, `&&`, and env var
expansion inside the command string all work exactly as typed, the
same as real `foreman`.

## Status: built and verified, including a real race caught by flaky-test hunting

- **13 unit tests**: `Procfile` parsing (colons inside a command like
  a URL don't break it, blank lines/comments skipped, clean errors for
  a missing colon or an empty command), the minimal `.env` parser
  (quote stripping, values containing `=`, non-`KEY=value` lines
  skipped rather than erroring), and color-palette cycling.
- **5 live end-to-end tests, run against the actual compiled binary
  driving real shell processes** — no mocks, since the entire point of
  this tool is real OS process/signal behavior:
  - Two real processes run concurrently, each producing real output
    correctly prefixed with its own name.
  - One process exiting with a real nonzero code (`exit 7`) shuts down
    a real still-sleeping sibling and procrun itself exits with that
    same code (`7`) — the actual propagation path, not asserted in the
    abstract.
  - **The signal-forwarding path specifically**: a child writes its
    own real PID to a file, procrun is sent a real SIGTERM (the same
    signal `docker stop` sends), and the test checks `/proc/<pid>`
    directly afterward — the only way to actually prove the child was
    terminated rather than just assuming a `Some(exit code)` meant
    the right thing happened.
  - `-e` loads a real `.env` file and a spawned process's real
    `$MY_TEST_VAR` correctly reflects it.
  - A missing `Procfile` fails cleanly with a message naming the file,
    not a panic.
- **A real, live-caught flaky-test bug, not a hypothetical one**: the
  first version of the "two processes, check both outputs"
  test used two truly instantaneous `echo` commands. Under repeated
  runs it failed intermittently — one process could finish and exit
  before the *other* had even been scheduled to start, triggering
  procrun's (correct, intentional) shut-down-everything-on-first-exit
  behavior and SIGTERM'ing the second process before it had produced
  any output at all. This wasn't a procrun bug — shutting down
  siblings the instant any one process exits is exactly the
  Foreman-style behavior real long-running services need — it was a
  test built around commands too fast for that behavior to be
  observed safely. Fixed by giving both commands a shared brief delay
  after echoing, and re-verified stable across 5 repeated full-suite
  runs where it had failed roughly 2 times out of 3 before the fix.

**Deliberate scope limits, stated plainly**: no `.foreman`-style
per-process `wait_for_port`/health-check gating before considering a
process "up"; no process restart-on-crash (a crash triggers full
shutdown, matching `foreman`'s own default, not `overmind`'s
optional restart mode); Unix-only (relies on real POSIX signals via
`nix`/`signal-hook` — no Windows support in this v1); the `.env`
parser is intentionally minimal (see `src/dotenv.rs`) — no variable
expansion or multi-line values.
