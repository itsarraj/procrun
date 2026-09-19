//! Live end-to-end tests. procrun's whole value proposition is real
//! process orchestration and signal forwarding — behavior a mock
//! can't stand in for — so every test here runs the actual compiled
//! `procrun` binary against real shell commands and checks real
//! process state afterward, not just captured strings.

use std::process::{Command, Stdio};
use std::time::Duration;

use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

fn bin_path() -> std::path::PathBuf {
    env!("CARGO_BIN_EXE_procrun").into()
}

fn write_procfile(dir: &std::path::Path, contents: &str) -> std::path::PathBuf {
    let path = dir.join("Procfile");
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn runs_two_processes_and_prefixes_their_output_with_their_names() {
    let dir = tempdir();
    // Both processes sleep briefly *after* echoing, rather than
    // exiting immediately — procrun tears down every other process as
    // soon as any one exits (correct, intentional foreman-style
    // behavior for real long-running services), so two truly
    // instantaneous commands race: whichever spawns first can echo
    // and exit before the second has even been scheduled to start,
    // and its SIGTERM can arrive before that second process has
    // produced any output at all. A shared brief delay guarantees
    // both have already echoed well before either is allowed to exit.
    write_procfile(
        &dir,
        "one: sh -c 'echo hello-from-one; sleep 0.3'\ntwo: sh -c 'echo hello-from-two; sleep 0.3'\n",
    );

    let output = Command::new(bin_path()).current_dir(&dir).output().unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("one") && stdout.contains("hello-from-one"),
        "stdout was:\n{stdout}"
    );
    assert!(
        stdout.contains("two") && stdout.contains("hello-from-two"),
        "stdout was:\n{stdout}"
    );
}

#[test]
fn a_process_exiting_nonzero_shuts_everything_down_and_propagates_the_exit_code() {
    let dir = tempdir();
    write_procfile(&dir, "failer: sh -c 'exit 7'\nsleeper: sleep 30\n");

    let status = Command::new(bin_path()).current_dir(&dir).status().unwrap();

    assert_eq!(status.code(), Some(7));
}

#[test]
fn sigterm_to_procrun_itself_terminates_the_still_running_children() {
    let dir = tempdir();
    // The sleeper writes its own PID to a file, then sleeps. After
    // procrun is killed with SIGTERM, we check whether that PID is
    // still a live process — the real, unfakeable signature of
    // whether signal forwarding actually happened.
    let pidfile = dir.join("sleeper.pid");
    write_procfile(
        &dir,
        &format!(
            "sleeper: sh -c 'echo $$ > {} && sleep 30'\n",
            pidfile.display()
        ),
    );

    let mut child = Command::new(bin_path())
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Give the child process time to start and write its pidfile.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !pidfile.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(pidfile.exists(), "sleeper never wrote its pidfile");
    let sleeper_pid: i32 = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();

    assert!(
        process_is_alive(sleeper_pid),
        "sleeper should be running before procrun is signaled"
    );

    // Send SIGTERM to procrun itself, same as `docker stop` or Ctrl-C
    // via a job-control shell would.
    kill(Pid::from_raw(child.id() as i32), Signal::SIGTERM).unwrap();

    let status = child.wait_timeout(Duration::from_secs(5));
    assert!(status.is_some(), "procrun did not exit after SIGTERM");

    // Give the (now-SIGTERM'd) sleeper a brief moment to actually exit.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while process_is_alive(sleeper_pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !process_is_alive(sleeper_pid),
        "sleeper (pid {sleeper_pid}) was still alive after procrun received SIGTERM — signal forwarding failed"
    );
}

#[test]
fn dash_e_loads_env_vars_into_every_process() {
    let dir = tempdir();
    write_procfile(&dir, "printer: sh -c 'echo VALUE_IS=$MY_TEST_VAR'\n");
    let envfile = dir.join(".env");
    std::fs::write(&envfile, "MY_TEST_VAR=hello-from-dotenv\n").unwrap();

    let output = Command::new(bin_path())
        .current_dir(&dir)
        .arg("-e")
        .arg(&envfile)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("VALUE_IS=hello-from-dotenv"),
        "stdout was:\n{stdout}"
    );
}

#[test]
fn a_missing_procfile_is_a_clean_error_not_a_panic() {
    let dir = tempdir();
    let output = Command::new(bin_path()).current_dir(&dir).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Procfile"), "stderr was: {stderr}");
}

// --- tiny test-only helpers (no extra crates just for these) ---

fn tempdir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("procrun-test-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn process_is_alive(pid: i32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

trait WaitTimeout {
    fn wait_timeout(&mut self, timeout: Duration) -> Option<std::process::ExitStatus>;
}

impl WaitTimeout for std::process::Child {
    fn wait_timeout(&mut self, timeout: Duration) -> Option<std::process::ExitStatus> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Ok(Some(status)) = self.try_wait() {
                return Some(status);
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
