use std::process::ExitCode;

const USAGE: &str = "usage: cusco hook <claude|codex>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("hook") => {
            if let Some(client) = args.get(1) {
                cusco::hook::run_hook(client, std::io::stdin().lock(), &cusco::hook::default_dir());
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
