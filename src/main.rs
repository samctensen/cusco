use cusco::launchd;
use cusco::server::{self, API_SOCKET, Config, SUBSCRIBER_QUEUE, default_dir};
use cusco::tree::STALE_AFTER;
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage:
  cusco setup [claude] [codex] [grok] [muse] [--remove]
  cusco start
  cusco stop
  cusco autostart [on|off]
  cusco serve
  cusco status [--json] [--requests] [--path <path>] [--depth <n>]
  cusco watch [--json] [--requests] [--path <path>] [--depth <n>]
  cusco visualize
  cusco hook <claude|codex|grok|muse>
  cusco --version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("hook") => {
            if let Some(client) = args.get(1) {
                cusco::hook::run_hook(client, std::io::stdin().lock(), &default_dir());
            }
            ExitCode::SUCCESS
        }
        Some("setup") => setup(&args[1..]),
        Some("start") if args.len() == 1 => start(),
        Some("stop") if args.len() == 1 => stop(),
        Some("autostart") => autostart(&args[1..]),
        Some("serve") if args.len() == 1 => serve(),
        Some("status") => query("get", &args[1..]),
        Some("watch") => query("subscribe", &args[1..]),
        Some("visualize") if args.len() == 1 => visualize(),
        Some("--version" | "-V") => {
            println!("cusco {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}

/// Adds Cusco's hook to the named clients, or to every client installed on
/// this machine when none are named.
fn setup(args: &[String]) -> ExitCode {
    let remove = args.iter().any(|arg| arg == "--remove");
    let named: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|arg| *arg != "--remove")
        .collect();
    let targets = cusco::setup::targets();
    if named
        .iter()
        .any(|name| !targets.iter().any(|target| target.client == *name))
    {
        return usage();
    }
    let chosen: Vec<_> = targets
        .into_iter()
        .filter(|target| {
            if named.is_empty() {
                target.home.exists()
            } else {
                named.contains(&target.client)
            }
        })
        .collect();
    if chosen.is_empty() {
        eprintln!("cusco: found no supported agents to set up");
        return ExitCode::FAILURE;
    }
    let executable = match cusco::setup::stable_executable() {
        Ok(executable) => executable,
        Err(error) => {
            eprintln!("cusco: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut code = ExitCode::SUCCESS;
    for target in chosen {
        let command = cusco::setup::hook_command(&executable, target.client);
        let path = target.path.display();
        match cusco::setup::apply(&target, &command, remove) {
            Ok(cusco::setup::Outcome::Added) => {
                println!("{}: added hooks to {path}", target.client);
                if target.client == "codex" {
                    println!("codex: open Codex and trust the new hooks with /hooks");
                }
            }
            Ok(cusco::setup::Outcome::Removed) => {
                println!("{}: removed hooks from {path}", target.client);
            }
            Ok(cusco::setup::Outcome::Unchanged) => {
                println!("{}: nothing to change in {path}", target.client);
            }
            Err(error) => {
                eprintln!("{}: {error}", target.client);
                code = ExitCode::FAILURE;
            }
        }
    }
    code
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn serve() -> ExitCode {
    let dir = default_dir();
    let config = Config {
        dir: dir.clone(),
        stale_after: STALE_AFTER,
        subscriber_queue: SUBSCRIBER_QUEUE,
        executables: cusco::process::ProcessRule::defaults(),
        claude_sessions: Some(cusco::registry::default_dir()),
    };
    match server::serve(config) {
        Ok(server) => {
            eprintln!("cusco: serving {}", dir.display());
            server.wait();
            eprintln!("cusco: stopped");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("cusco: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the service as a launchd job: in the background, and again if it
/// crashes, until `cusco stop` or logout.
fn start() -> ExitCode {
    if !launchd_managed() {
        return unmanaged();
    }
    let dir = default_dir();
    let socket = dir.join(API_SOCKET);
    if UnixStream::connect(&socket).is_ok() {
        if launchd::loaded(launchd::LABEL) {
            println!("cusco: already running");
            return ExitCode::SUCCESS;
        }
        if !launchd::brew_loaded() {
            eprintln!("cusco: already running outside launchd; stop it with cusco stop first");
            return ExitCode::FAILURE;
        }
        // Replace the job `brew services` started with Cusco's own.
        if !stop_service(&socket) {
            return ExitCode::FAILURE;
        }
    }
    let started = cusco::setup::stable_executable().and_then(|executable| {
        server::create_dir(&dir)?;
        launchd::start(&executable, &dir)
    });
    if let Err(error) = started {
        eprintln!("cusco: {error}");
        return ExitCode::FAILURE;
    }
    if wait_until(|| UnixStream::connect(&socket).is_ok()) {
        println!("cusco: started");
        return ExitCode::SUCCESS;
    }
    let log = dir.join(launchd::LOG);
    eprintln!(
        "cusco: not answering after 5 seconds; see {}",
        log.display()
    );
    ExitCode::FAILURE
}

/// Stops the service and unloads its launchd job, so it stays stopped until
/// `cusco start`, or the next login when autostart is on.
fn stop() -> ExitCode {
    let socket = default_dir().join(API_SOCKET);
    let running = UnixStream::connect(&socket).is_ok();
    let managed = launchd_managed();
    let loaded = managed && (launchd::loaded(launchd::LABEL) || launchd::brew_loaded());
    if !running && !loaded {
        eprintln!("cusco is not running: no socket at {}", socket.display());
        return ExitCode::FAILURE;
    }
    // Asked over the socket, the service closes its connections and removes
    // its sockets before exiting, which launchd's SIGTERM wouldn't let it do.
    if running && !stop_service(&socket) {
        return ExitCode::FAILURE;
    }
    if managed && let Err(error) = launchd::stop() {
        eprintln!("cusco: {error}");
        return ExitCode::FAILURE;
    }
    println!("cusco: stopped");
    ExitCode::SUCCESS
}

/// Asks the running service to stop, then waits until its socket stops
/// answering.
fn stop_service(socket: &Path) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return true;
    };
    let _ = writeln!(stream, "{}", json!({"v": 1, "op": "stop"}));
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let message: Value = serde_json::from_str(&line).unwrap_or_default();
        if message["type"] == "error" {
            let reason = message["message"].as_str().unwrap_or_default();
            eprintln!("cusco: the service refused to stop: {reason}");
            return false;
        }
    }
    if wait_until(|| UnixStream::connect(socket).is_err()) {
        return true;
    }
    eprintln!("cusco: still running after 5 seconds");
    false
}

/// launchd runs the copy of Cusco that serves the default directory. One
/// pointed elsewhere with `CUSCO_DIR` runs with `cusco serve`, and must not
/// replace or stop launchd's.
fn launchd_managed() -> bool {
    std::env::var_os("CUSCO_DIR").is_none()
}

fn unmanaged() -> ExitCode {
    eprintln!("cusco: CUSCO_DIR is set; run this copy with cusco serve");
    ExitCode::FAILURE
}

fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if done() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

/// Prints whether the service starts at login, or turns that on or off.
fn autostart(args: &[String]) -> ExitCode {
    let changed = match args {
        [] => {
            println!("{}", if launchd::autostart() { "on" } else { "off" });
            return ExitCode::SUCCESS;
        }
        [on_off] if on_off == "on" || on_off == "off" => {
            if !launchd_managed() {
                return unmanaged();
            }
            if on_off == "off" {
                launchd::disable_autostart()
            } else {
                cusco::setup::stable_executable().and_then(|executable| {
                    let dir = default_dir();
                    server::create_dir(&dir)?;
                    launchd::enable_autostart(&executable, &dir)
                })
            }
        }
        _ => return usage(),
    };
    match changed {
        Ok(()) if args[0] == "on" => println!("cusco: autostart on; Cusco starts at every login"),
        Ok(()) => println!("cusco: autostart off; Cusco won't start at login"),
        Err(error) => {
            eprintln!("cusco: {error}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// Sends one API request built from the flags and prints every line of the
/// answer as it arrives.
fn query(op: &str, flags: &[String]) -> ExitCode {
    let Some(request) = build_request(op, flags) else {
        return usage();
    };
    let Some(mut stream) = connect() else {
        return ExitCode::FAILURE;
    };
    if writeln!(stream, "{request}").is_err() {
        eprintln!("cusco: the service closed the connection");
        return ExitCode::FAILURE;
    }
    let mut stdout = std::io::stdout().lock();
    let mut code = ExitCode::SUCCESS;
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else {
            break;
        };
        if line.contains(r#""type":"error""#) {
            code = ExitCode::FAILURE;
        }
        if writeln!(stdout, "{line}")
            .and_then(|()| stdout.flush())
            .is_err()
        {
            break;
        }
    }
    code
}

/// Draws a live view of every agent until Ctrl-C.
fn visualize() -> ExitCode {
    let Some(stream) = connect() else {
        return ExitCode::FAILURE;
    };
    match cusco::visualize::run(stream) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cusco: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Connects to the running service, or says that it isn't running.
fn connect() -> Option<UnixStream> {
    let socket = default_dir().join(API_SOCKET);
    let stream = UnixStream::connect(&socket);
    if stream.is_err() {
        eprintln!("cusco is not running: no socket at {}", socket.display());
    }
    stream.ok()
}

fn build_request(op: &str, flags: &[String]) -> Option<Value> {
    let mut request = Map::new();
    request.insert("v".into(), json!(1));
    request.insert("op".into(), json!(op));
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        match flag.as_str() {
            "--json" => {}
            "--requests" => {
                request.insert("view".into(), json!("requests"));
            }
            "--path" => {
                request.insert("path".into(), json!(flags.next()?));
            }
            "--depth" => {
                let depth: u32 = flags.next()?.parse().ok()?;
                request.insert("depth".into(), json!(depth));
            }
            _ => return None,
        }
    }
    Some(Value::Object(request))
}
