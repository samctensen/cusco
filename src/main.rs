use cusco::server::{self, API_SOCKET, Config, SUBSCRIBER_QUEUE, default_dir};
use cusco::tree::STALE_AFTER;
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

const USAGE: &str = "usage:
  cusco serve
  cusco status [--json] [--requests] [--path <path>] [--depth <n>]
  cusco watch [--json] [--requests] [--path <path>] [--depth <n>]
  cusco hook <claude|codex>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("hook") => {
            if let Some(client) = args.get(1) {
                cusco::hook::run_hook(client, std::io::stdin().lock(), &default_dir());
            }
            ExitCode::SUCCESS
        }
        Some("serve") if args.len() == 1 => serve(),
        Some("status") => query("get", &args[1..]),
        Some("watch") => query("subscribe", &args[1..]),
        _ => usage(),
    }
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
        executables: vec![("claude", "claude".into()), ("codex", "codex".into())],
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
