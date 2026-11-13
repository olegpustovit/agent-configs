//! `cargo t`: the only way tests are run in this workspace.
//!
//! It builds the test executables once, then runs every test case as its own process under a hard
//! deadline, kills anything that overruns, prints the slowest cases and appends `target/t-times.tsv`.
//!
//!   cargo t -p core                  focused crate
//!   cargo t -p core rename           cases whose name contains "rename"
//!   T_SLOW=1 cargo t --workspace     qualification: long deadlines, also runs #[ignore] cases
//!   cargo t --touch crates/core/src/lib.rs --budget 5 -p api
//!                                    time an incremental rebuild after touching one file
//!
//! Deadlines per case: unit tests (inside `src/`) 2 s, integration tests (`tests/`) 5 s.
//! A whole run stops at 60 s. `T_SLOW=1` raises these to 60 s per case and 600 s per run.
//! Never raise a deadline to make a slow test pass: shrink the test or move it to the slow tier.

use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const UNIT_DEADLINE: Duration = Duration::from_secs(2);
const INTEGRATION_DEADLINE: Duration = Duration::from_secs(5);
const RUN_DEADLINE: Duration = Duration::from_secs(60);
const SLOW_CASE_DEADLINE: Duration = Duration::from_secs(60);
const SLOW_RUN_DEADLINE: Duration = Duration::from_secs(600);
const BUILD_DEADLINE: Duration = Duration::from_secs(900);
const POLL: Duration = Duration::from_millis(10);

struct Exe {
    path: PathBuf,
    krate: String,
    integration: bool,
}

struct Case {
    exe: PathBuf,
    krate: String,
    name: String,
    deadline: Duration,
    serial: bool,
}

struct Outcome {
    krate: String,
    name: String,
    elapsed: Duration,
    status: &'static str, // ok | FAIL | TIMEOUT | NOT RUN
    log: PathBuf,
}

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let code = if args.first().is_some_and(|a| a == "--touch") { touch(&args[1..]) } else { run(&args) };
    ExitCode::from(code)
}

fn run(args: &[OsString]) -> u8 {
    let slow = env::var("T_SLOW").is_ok_and(|v| v == "1");
    let (cargo_args, filter) = split_args(args);
    let started = Instant::now();

    let exes = match build(&cargo_args) {
        Ok(exes) => exes,
        Err(code) => return code,
    };
    let built = started.elapsed();

    let mut cases = Vec::new();
    for exe in &exes {
        let deadline = match (slow, exe.integration) {
            (true, _) => SLOW_CASE_DEADLINE,
            (false, true) => INTEGRATION_DEADLINE,
            (false, false) => UNIT_DEADLINE,
        };
        for name in list(&exe.path, slow) {
            if filter.as_deref().is_some_and(|f| !name.contains(f)) {
                continue;
            }
            let serial = name.split("::").any(|part| part.starts_with("serial_"));
            cases.push(Case { exe: exe.path.clone(), krate: exe.krate.clone(), name, deadline, serial });
        }
    }
    if cases.is_empty() {
        eprintln!("t: no test cases matched");
        return 1;
    }

    let target = target_dir();
    let logs = target.join("t-logs");
    let _ = fs::remove_dir_all(&logs);
    if let Err(e) = fs::create_dir_all(&logs) {
        eprintln!("t: cannot create {}: {e}", logs.display());
        return 1;
    }

    let run_deadline = Instant::now() + if slow { SLOW_RUN_DEADLINE } else { RUN_DEADLINE };
    let (serial, parallel): (Vec<Case>, Vec<Case>) = cases.into_iter().partition(|c| c.serial);
    let workers = thread::available_parallelism().map_or(4, usize::from);
    let mut outcomes = run_batch(&parallel, workers, run_deadline, &logs, 0, slow);
    outcomes.extend(run_batch(&serial, 1, run_deadline, &logs, parallel.len(), slow));

    report(&outcomes, built, started.elapsed(), &target)
}

/// Everything before a bare word is passed to cargo; the first bare word is the name filter.
fn split_args(args: &[OsString]) -> (Vec<OsString>, Option<String>) {
    let takes_value = ["-p", "--package", "--test", "--features", "--bin", "--exclude", "--profile"];
    let mut cargo = Vec::new();
    let mut filter = None;
    let mut i = 0;
    while i < args.len() {
        let text = args[i].to_string_lossy().into_owned();
        if text.starts_with('-') {
            cargo.push(args[i].clone());
            if takes_value.contains(&text.as_str()) && i + 1 < args.len() {
                i += 1;
                cargo.push(args[i].clone());
            }
        } else if filter.is_none() {
            filter = Some(text);
        }
        i += 1;
    }
    (cargo, filter)
}

fn build(cargo_args: &[OsString]) -> Result<Vec<Exe>, u8> {
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.args(["test", "--no-run", "--message-format=json"]).args(cargo_args);
    command.stdout(Stdio::piped()).stderr(Stdio::inherit());
    let mut child = spawn_group(&mut command).map_err(|e| {
        eprintln!("t: cannot start cargo: {e}");
        1
    })?;
    let stdout = child.stdout.take();
    let reader = thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut out) = stdout {
            let _ = std::io::Read::read_to_string(&mut out, &mut text);
        }
        text
    });
    let Some(ok) = wait_until(&mut child, Instant::now() + BUILD_DEADLINE) else {
        eprintln!("t: build killed after {} s", BUILD_DEADLINE.as_secs());
        return Err(124);
    };
    let text = reader.join().unwrap_or_default();
    if !ok {
        return Err(101);
    }
    let mut exes = Vec::new();
    for line in text.lines().filter(|l| l.contains("\"reason\":\"compiler-artifact\"")) {
        let Some(path) = json_str(line, "\"executable\":") else { continue };
        if !line.contains("\"test\":true") {
            continue;
        }
        let target = line.find("\"target\":").map_or(line, |at| &line[at..]);
        let kind = json_str(target, "\"kind\":[").unwrap_or_default();
        let krate = json_str(target, "\"name\":").unwrap_or_default();
        exes.push(Exe { path: PathBuf::from(path), krate, integration: kind == "test" });
    }
    Ok(exes)
}

/// The string value that follows `key` in one line of cargo's JSON. Enough for paths and names.
fn json_str(line: &str, key: &str) -> Option<String> {
    let rest = &line[line.find(key)? + key.len()..];
    let rest = rest.strip_prefix('"')?;
    let mut value = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(value),
            '\\' => value.push(chars.next()?),
            c => value.push(c),
        }
    }
    None
}

/// Case names in one test executable. `#[ignore]` cases are the slow tier: listed only when `include_ignored`.
fn list(exe: &Path, include_ignored: bool) -> Vec<String> {
    let names = |extra: &[&str]| -> Vec<String> {
        let Ok(output) = Command::new(exe).args(["--list", "--format", "terse"]).args(extra).output() else { return Vec::new() };
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.strip_suffix(": test").map(str::to_owned))
            .collect()
    };
    let all = names(&[]);
    if include_ignored {
        return all;
    }
    let ignored = names(&["--ignored"]);
    all.into_iter().filter(|name| !ignored.contains(name)).collect()
}

fn run_batch(cases: &[Case], workers: usize, run_deadline: Instant, logs: &Path, offset: usize, slow: bool) -> Vec<Outcome> {
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let outcomes = Mutex::new(Vec::with_capacity(cases.len()));
    thread::scope(|scope| {
        for _ in 0..workers.min(cases.len()) {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(case) = cases.get(index) else { break };
                let log = logs.join(format!("{}.log", offset + index));
                let outcome = if stop.load(Ordering::Relaxed) || Instant::now() >= run_deadline {
                    stop.store(true, Ordering::Relaxed);
                    Outcome { krate: case.krate.clone(), name: case.name.clone(), elapsed: Duration::ZERO, status: "NOT RUN", log }
                } else {
                    run_case(case, run_deadline, log, slow)
                };
                outcomes.lock().unwrap_or_else(|p| p.into_inner()).push(outcome);
            });
        }
    });
    outcomes.into_inner().unwrap_or_else(|p| p.into_inner())
}

fn run_case(case: &Case, run_deadline: Instant, log: PathBuf, slow: bool) -> Outcome {
    let started = Instant::now();
    let outcome = |status| Outcome { krate: case.krate.clone(), name: case.name.clone(), elapsed: started.elapsed(), status, log: log.clone() };
    let Ok(file) = File::create(&log) else { return outcome("FAIL") };
    let Ok(file_err) = file.try_clone() else { return outcome("FAIL") };
    let mut command = Command::new(&case.exe);
    command.args(["--exact", &case.name, "--nocapture", "--test-threads=1"]);
    if slow {
        command.arg("--include-ignored");
    }
    command.stdin(Stdio::null()).stdout(file).stderr(file_err);
    let Ok(mut child) = spawn_group(&mut command) else { return outcome("FAIL") };
    let deadline = (started + case.deadline).min(run_deadline);
    match wait_until(&mut child, deadline) {
        Some(true) => outcome("ok"),
        Some(false) => outcome("FAIL"),
        None => outcome("TIMEOUT"),
    }
}

fn spawn_group(command: &mut Command) -> std::io::Result<Child> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn()
}

/// Waits for the child. `None` means the deadline passed and the child's whole process group was killed.
fn wait_until(child: &mut Child, deadline: Instant) -> Option<bool> {
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status.success());
        }
        if Instant::now() >= deadline {
            #[cfg(unix)]
            let _ = Command::new("/bin/kill").args(["-KILL", "--", &format!("-{}", child.id())]).stderr(Stdio::null()).status();
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(POLL);
    }
}

fn report(outcomes: &[Outcome], built: Duration, wall: Duration, target: &Path) -> u8 {
    let bad: Vec<&Outcome> = outcomes.iter().filter(|o| o.status != "ok").collect();
    for outcome in &bad {
        eprintln!("\n---- {}::{} {} after {:.3}s", outcome.krate, outcome.name, outcome.status, outcome.elapsed.as_secs_f64());
        if let Ok(text) = fs::read_to_string(&outcome.log) {
            eprint!("{text}");
        }
    }
    let mut slowest: Vec<&Outcome> = outcomes.iter().collect();
    slowest.sort_by_key(|outcome| std::cmp::Reverse(outcome.elapsed));
    eprintln!(
        "\nt: {} cases, {} failed; build {:.2}s, total {:.2}s",
        outcomes.len(),
        bad.len(),
        built.as_secs_f64(),
        wall.as_secs_f64()
    );
    for outcome in slowest.iter().take(5) {
        eprintln!("t: slowest {:>7.3}s {}::{} {}", outcome.elapsed.as_secs_f64(), outcome.krate, outcome.name, outcome.status);
    }
    let rows: Vec<String> = outcomes
        .iter()
        .map(|o| format!("{}\t{}\t{}\t{:.3}\t{}", unix_now(), o.krate, o.name, o.elapsed.as_secs_f64(), o.status))
        .collect();
    append(&target.join("t-times.tsv"), &rows);
    if bad.is_empty() { 0 } else { 1 }
}

/// `cargo t --touch <file> [--budget <seconds>] [cargo build args]`: bump one source file's mtime, then time the rebuild.
fn touch(args: &[OsString]) -> u8 {
    let Some(file) = args.first().map(PathBuf::from) else {
        eprintln!("t: --touch needs a source file");
        return 2;
    };
    let mut budget = None;
    let mut cargo_args = Vec::new();
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        if arg == "--budget" {
            budget = rest.next().and_then(|v| v.to_string_lossy().parse::<f64>().ok());
        } else {
            cargo_args.push(arg.clone());
        }
    }
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let build = |quiet: bool| {
        let mut command = Command::new(&cargo);
        command.arg("build").args(&cargo_args);
        if quiet {
            command.stderr(Stdio::null());
        }
        command.status().is_ok_and(|s| s.success())
    };
    if !build(false) {
        return 101; // warm the cache first, so only the touched file's rebuild is timed
    }
    let touched = OpenOptions::new().write(true).open(&file).and_then(|f| f.set_modified(SystemTime::now()));
    if let Err(e) = touched {
        eprintln!("t: cannot touch {}: {e}", file.display());
        return 1;
    }
    let started = Instant::now();
    if !build(true) {
        return 101;
    }
    let secs = started.elapsed().as_secs_f64();
    let over = budget.is_some_and(|b| secs > b);
    eprintln!("t: rebuild after touching {} took {secs:.2}s{}", file.display(), if over { " OVER BUDGET" } else { "" });
    append(&target_dir().join("build-times.tsv"), &[format!("{}\t{}\t{secs:.3}", unix_now(), file.display())]);
    u8::from(over)
}

fn target_dir() -> PathBuf {
    env::var_os("CARGO_TARGET_DIR").map_or_else(|| PathBuf::from("target"), PathBuf::from)
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn append(path: &Path, rows: &[String]) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        for row in rows {
            let _ = writeln!(file, "{row}");
        }
    }
}
