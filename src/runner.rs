use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

use crate::color::{color_for_index, colorize};
use crate::procfile::ProcessEntry;

/// How long to wait for children to exit gracefully after SIGTERM
/// before escalating to SIGKILL.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

enum Event {
    Exited {
        name: String,
        pid: u32,
        code: Option<i32>,
    },
    ShutdownSignal,
}

/// Runs every entry in `entries` concurrently, prefixing each line of
/// its stdout/stderr with a colored `name |`. If any process exits, or
/// if procrun itself receives SIGINT/SIGTERM, every other process is
/// sent SIGTERM (escalating to SIGKILL after `SHUTDOWN_GRACE` for
/// stragglers) before this function returns.
///
/// Returns the process exit code procrun itself should use: the
/// triggering child's own exit code if a child crashed or exited on
/// its own, or `0` for a deliberate SIGINT/SIGTERM shutdown.
pub fn run(entries: Vec<ProcessEntry>, extra_env: &HashMap<String, String>) -> i32 {
    let name_width = entries.iter().map(|e| e.name.len()).max().unwrap_or(0);
    let output_lock = Arc::new(Mutex::new(()));
    let (tx, rx): (Sender<Event>, Receiver<Event>) = mpsc::channel();

    let mut pids: Vec<(String, u32)> = Vec::new();

    for (index, entry) in entries.iter().enumerate() {
        let color = color_for_index(index);
        let mut child = match Command::new("sh")
            .arg("-c")
            .arg(&entry.command)
            .envs(extra_env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("procrun: failed to start '{}': {e}", entry.name);
                // Whatever's already running needs to be torn down too,
                // not left orphaned just because a later entry failed.
                for (_, pid) in &pids {
                    let _ = kill(Pid::from_raw(*pid as i32), Signal::SIGTERM);
                }
                return 1;
            }
        };

        let pid = child.id();
        pids.push((entry.name.clone(), pid));

        spawn_reader(
            &mut child,
            entry.name.clone(),
            name_width,
            color,
            Arc::clone(&output_lock),
        );
        spawn_waiter(child, entry.name.clone(), pid, tx.clone());
    }

    spawn_signal_watcher(tx.clone());
    drop(tx); // the loop below only needs the clones held by threads

    let mut exited: HashSet<u32> = HashSet::new();
    // Only the first event received drives the shutdown decision — the
    // `terminate_others*` calls below have their own internal loop to
    // drain any further `Exited` events that arrive while everything
    // else is being torn down.
    let exit_code = match rx.recv() {
        Ok(Event::Exited { name, pid, code }) => {
            exited.insert(pid);
            log_note(&format!(
                "'{name}' exited ({}) — shutting down the rest",
                code.map(|c| c.to_string())
                    .unwrap_or_else(|| "signaled".to_string())
            ));
            terminate_others(&pids, pid, &mut exited, &rx);
            code.unwrap_or(1)
        }
        Ok(Event::ShutdownSignal) => {
            log_note("received shutdown signal — stopping all processes");
            terminate_others_all(&pids, &mut exited, &rx);
            0
        }
        Err(_) => 1,
    };

    exit_code
}

fn log_note(message: &str) {
    eprintln!("{}", colorize(&format!("procrun | {message}"), "\x1b[90m"));
}

fn terminate_others(
    pids: &[(String, u32)],
    already_exited_pid: u32,
    exited: &mut HashSet<u32>,
    rx: &Receiver<Event>,
) {
    for (_, pid) in pids {
        if *pid != already_exited_pid {
            let _ = kill(Pid::from_raw(*pid as i32), Signal::SIGTERM);
        }
    }
    wait_for_exit_or_kill(pids, exited, rx);
}

fn terminate_others_all(pids: &[(String, u32)], exited: &mut HashSet<u32>, rx: &Receiver<Event>) {
    for (_, pid) in pids {
        let _ = kill(Pid::from_raw(*pid as i32), Signal::SIGTERM);
    }
    wait_for_exit_or_kill(pids, exited, rx);
}

fn wait_for_exit_or_kill(pids: &[(String, u32)], exited: &mut HashSet<u32>, rx: &Receiver<Event>) {
    let deadline = std::time::Instant::now() + SHUTDOWN_GRACE;
    while exited.len() < pids.len() {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok(Event::Exited { pid, .. }) => {
                exited.insert(pid);
            }
            Ok(Event::ShutdownSignal) => {} // already shutting down; ignore a second signal
            Err(_) => break,
        }
    }
    for (_, pid) in pids {
        if !exited.contains(pid) {
            let _ = kill(Pid::from_raw(*pid as i32), Signal::SIGKILL);
        }
    }
}

fn spawn_reader(
    child: &mut Child,
    name: String,
    name_width: usize,
    color: &'static str,
    lock: Arc<Mutex<()>>,
) {
    if let Some(stdout) = child.stdout.take() {
        spawn_line_printer(stdout, name.clone(), name_width, color, Arc::clone(&lock));
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_line_printer(stderr, name, name_width, color, lock);
    }
}

fn spawn_line_printer<R: io::Read + Send + 'static>(
    reader: R,
    name: String,
    name_width: usize,
    color: &'static str,
    lock: Arc<Mutex<()>>,
) {
    thread::spawn(move || {
        let buf = BufReader::new(reader);
        for line in buf.lines() {
            let Ok(line) = line else { break };
            let _guard = lock.lock().unwrap();
            let prefix = colorize(&format!("{name:name_width$} |"), color);
            println!("{prefix} {line}");
            let _ = io::stdout().flush();
        }
    });
}

fn spawn_waiter(mut child: Child, name: String, pid: u32, tx: Sender<Event>) {
    thread::spawn(move || {
        let status = child.wait();
        let code = status.ok().and_then(|s| s.code());
        let _ = tx.send(Event::Exited { name, pid, code });
    });
}

fn spawn_signal_watcher(tx: Sender<Event>) {
    thread::spawn(move || {
        if let Ok(mut signals) = signal_hook::iterator::Signals::new([
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGTERM,
        ]) {
            if signals.forever().next().is_some() {
                let _ = tx.send(Event::ShutdownSignal);
            }
        }
    });
}
