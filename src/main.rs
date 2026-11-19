use cusco::server::{self, API_SOCKET, Config, SUBSCRIBER_QUEUE, default_dir};
use cusco::tree::STALE_AFTER;
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

const USAGE: &str = "usage:
  cusco setup [claude] [codex] [grok] [muse] [--remove]
  cusco serve
  cusco status [--json] [--requests] [--path <path>] [--depth <n>]
  cusco watch [--json] [--requests] [--path <path>] [--depth <n>]
  cusco hook <claude|codex>
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
        Some("serve") if args.len() == 1 => serve(),
        Some("status") => query("get", &args[1..]),
        Some("watch") => query("subscribe", &args[1..]),
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
    let executable = match cusco::setup::hook_executable() {
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
    };
    match server::serve(config) {
        Ok(server) => {
            eprintln!("cusco: serving {}", dir.display());
            server.wait();
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("cusco: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Sends one API request built from the flags and prints every line of the
/// answer as it arrives.
fn query(op: &str, flags: &[String]) -> ExitCode {
    let Some(request) = build_request(op, flags) else {
        return usage();
    };
    let socket = default_dir().join(API_SOCKET);
    let Ok(mut stream) = UnixStream::connect(&socket) else {
        eprintln!("cusco is not running: no socket at {}", socket.display());
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
