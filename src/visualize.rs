//! `eywa visualize`: a live view of every agent Eywa can see, redrawn in the
//! terminal as it changes.

use crate::tree::Millis;
use serde_json::Value;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Rows by id, as the subscription last described them.
pub type Rows = BTreeMap<String, Value>;

const FRAME: Millis = 100;
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const STATES: [&str; 4] = ["needs_input", "working", "unknown", "idle"];

const PLAIN: &str = "";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn stop(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

/// Subscribes to everything on `stream` and redraws the view until Ctrl-C,
/// or until the service goes away.
pub fn run(mut stream: UnixStream) -> io::Result<()> {
    writeln!(stream, r#"{{"v":1,"op":"subscribe","path":"/","depth":8}}"#)?;
    let (sender, messages) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if let Ok(message) = serde_json::from_str::<Value>(&line)
                && sender.send(message).is_err()
            {
                break;
            }
        }
    });
    let handler = stop as extern "C" fn(libc::c_int) as libc::sighandler_t;
    // SAFETY: the handler only stores to an atomic, which is signal-safe.
    unsafe {
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGTERM, handler);
    }
    let mut out = io::stdout().lock();
    // Switch to the alternate screen and hide the cursor; both are undone below.
    out.write_all(b"\x1b[?1049h\x1b[?25l")?;
    let result = animate(&messages, &mut out);
    out.write_all(b"\x1b[?25h\x1b[?1049l")?;
    out.flush()?;
    result
}

fn animate(messages: &mpsc::Receiver<Value>, out: &mut impl Write) -> io::Result<()> {
    let mut rows = Rows::new();
    while !STOP.load(Ordering::Relaxed) {
        let wait = FRAME - now() % FRAME;
        match messages.recv_timeout(Duration::from_millis(wait)) {
            Ok(message) => {
                apply(&mut rows, message)?;
                for message in messages.try_iter() {
                    apply(&mut rows, message)?;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "the service stopped",
                ));
            }
        }
        let (width, height) = terminal_size();
        let mut screen = String::from("\x1b[H");
        for (index, line) in render(&rows, now(), width).lines().take(height).enumerate() {
            if index > 0 {
                screen.push_str("\r\n");
            }
            screen.push_str(line);
            screen.push_str("\x1b[K");
        }
        screen.push_str("\x1b[J");
        out.write_all(screen.as_bytes())?;
        out.flush()?;
    }
    Ok(())
}

fn apply(rows: &mut Rows, mut message: Value) -> io::Result<()> {
    match message["type"].as_str() {
        Some("snapshot") => {
            rows.clear();
            if let Value::Array(list) = message["rows"].take() {
                list.into_iter().for_each(|row| insert(rows, row));
            }
        }
        Some("upsert") => insert(rows, message["row"].take()),
        Some("remove") => {
            if let Some(id) = message["id"].as_str() {
                rows.remove(id);
            }
        }
        Some("error") => {
            let reason = message["message"].as_str().unwrap_or("request refused");
            return Err(io::Error::other(reason.to_string()));
        }
        _ => {}
    }
    Ok(())
}

fn insert(rows: &mut Rows, row: Value) {
    if let Some(id) = row["id"].as_str().map(String::from) {
        rows.insert(id, row);
    }
}

/// Draws `rows` as they stand at `now`, each line at most `width` columns.
pub fn render(rows: &Rows, now: Millis, width: usize) -> String {
    let view = View::new(rows, now);
    let mut lines = vec![view.header(), Line::default()];
    for client in view.children("/") {
        view.client(client, &mut lines);
        lines.push(Line::default());
    }
    lines.push(Line::default().add(DIM, "  ⌃C to quit"));
    let mut screen = String::new();
    for line in lines {
        line.write(width, &mut screen);
        screen.push('\n');
    }
    screen
}

struct View<'a> {
    rows: &'a Rows,
    children: BTreeMap<&'a str, Vec<&'a str>>,
    now: Millis,
    frame: usize,
}

impl<'a> View<'a> {
    fn new(rows: &'a Rows, now: Millis) -> Self {
        let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for id in rows.keys().filter(|id| *id != "/") {
            let parent = match id.rsplit_once('/') {
                Some(("", _)) | None => "/",
                Some((parent, _)) => parent,
            };
            children.entry(parent).or_default().push(id);
        }
        // Clients keep their order. Beneath them, whatever needs you comes
        // first, then the latest to change.
        for (parent, list) in &mut children {
            if *parent != "/" {
                list.sort_by_cached_key(|id| {
                    let row = &rows[*id];
                    (state(row) != "needs_input", Reverse(since(row)), *id)
                });
            }
        }
        View {
            rows,
            children,
            now,
            frame: (now / FRAME) as usize,
        }
    }

    fn children(&self, id: &str) -> &[&'a str] {
        self.children.get(id).map_or(&[], Vec::as_slice)
    }

    fn header(&self) -> Line {
        let line = Line::default().add(BOLD, "  eywa");
        let Some(machine) = self.rows.get("/") else {
            return line.add(DIM, " · connecting…");
        };
        let counts = &machine["counts"];
        let sessions = counts["sessions"].as_u64().unwrap_or(0);
        let plural = if sessions == 1 { "" } else { "s" };
        let mut line = line.add(DIM, format!(" · {sessions} session{plural}"));
        for state in STATES {
            let count = counts[state].as_u64().unwrap_or(0);
            if count > 0 {
                let what = match state {
                    "needs_input" if count > 1 => "need you",
                    _ => word(state),
                };
                line = line
                    .add(PLAIN, "   ")
                    .add(self.color(state), self.glyph(state))
                    .add(PLAIN, format!(" {count} {what}"));
            }
        }
        line
    }

    fn client(&self, id: &str, lines: &mut Vec<Line>) {
        let state = state(&self.rows[id]);
        let sessions = self.children(id);
        let summary = match sessions.len() {
            0 => "no sessions".to_string(),
            1 => "1 session".to_string(),
            count => format!("{count} sessions"),
        };
        lines.push(
            Line::default()
                .add(PLAIN, "  ")
                .add(self.color(state), self.glyph(state))
                .add(BOLD, format!(" {}", id.trim_start_matches('/')))
                .add(DIM, format!("  {summary}")),
        );
        self.branches(sessions, "  ", lines);
    }

    /// Draws `ids` as siblings in the tree, expanding each that isn't idle.
    fn branches(&self, ids: &[&str], indent: &str, lines: &mut Vec<Line>) {
        let column = ids
            .iter()
            .map(|id| {
                let (name, detail) = label(id, &self.rows[*id]);
                name.chars().count() + detail.chars().count()
            })
            .max()
            .unwrap_or(0);
        for (index, id) in ids.iter().enumerate() {
            let last = index + 1 == ids.len();
            let row = &self.rows[*id];
            let state = state(row);
            let (name, detail) = label(id, row);
            let gap = column - name.chars().count() - detail.chars().count() + 2;
            lines.push(
                Line::default()
                    .add(DIM, format!("{indent}{}", if last { "└─ " } else { "├─ " }))
                    .add(self.color(state), self.glyph(state))
                    .add(
                        if state == "idle" { DIM } else { PLAIN },
                        format!(" {name}"),
                    )
                    .add(DIM, detail)
                    .add(PLAIN, " ".repeat(gap))
                    .add(self.color(state), word(state))
                    .add(DIM, format!(" {}", elapsed(row, self.now))),
            );
            if state != "idle" {
                let indent = format!("{indent}{}", if last { "   " } else { "│  " });
                self.branches(self.children(id), &indent, lines);
            }
        }
    }

    /// Anything waiting on you pulses: bright for half a second, then dim.
    fn bright(&self) -> bool {
        (self.frame / 5).is_multiple_of(2)
    }

    fn color(&self, state: &str) -> &'static str {
        match state {
            "needs_input" if self.bright() => "\x1b[1;95m",
            "needs_input" => "\x1b[35m",
            "working" => "\x1b[32m",
            "unknown" => "\x1b[33m",
            _ => DIM,
        }
    }

    fn glyph(&self, state: &str) -> &'static str {
        match state {
            "needs_input" if self.bright() => "◆",
            "needs_input" => "◇",
            "working" => SPINNER[self.frame % SPINNER.len()],
            "unknown" => "?",
            _ => "○",
        }
    }
}

fn state(row: &Value) -> &str {
    row["state"].as_str().unwrap_or("unknown")
}

fn word(state: &str) -> &'static str {
    match state {
        "needs_input" => "needs you",
        "working" => "working",
        "idle" => "idle",
        _ => "unknown",
    }
}

/// A node's name and a dim detail after it, such as a session's folder and
/// short id.
fn label(id: &str, row: &Value) -> (String, String) {
    let segment = id.rsplit('/').next().unwrap_or(id);
    let (kind, key) = segment.split_once(':').unwrap_or((segment, ""));
    let short: String = key.chars().take(8).collect();
    match kind {
        "session" => {
            let folder = row["attrs"]["cwd"]
                .as_str()
                .and_then(|cwd| cwd.rsplit('/').find(|part| !part.is_empty()));
            (folder.unwrap_or("session").to_string(), format!(" {short}"))
        }
        "request" => {
            let reason = row["attrs"]["reason"].as_str().unwrap_or("request");
            (reason.replace('_', " "), String::new())
        }
        _ if short.is_empty() => (kind.to_string(), String::new()),
        _ => (kind.to_string(), format!(" {short}")),
    }
}

/// When the row entered its current state.
fn since(row: &Value) -> Option<Millis> {
    let since = humantime::parse_rfc3339(row["since"].as_str()?).ok()?;
    Some(since.duration_since(UNIX_EPOCH).ok()?.as_millis() as Millis)
}

fn elapsed(row: &Value, now: Millis) -> String {
    let Some(since) = since(row) else {
        return String::new();
    };
    let seconds = now.saturating_sub(since) / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        3600..86400 => format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60),
        _ => format!("{}d {}h", seconds / 86400, seconds % 86400 / 3600),
    }
}

/// Styled pieces of one line of the screen.
#[derive(Default)]
struct Line(Vec<(&'static str, String)>);

impl Line {
    /// Adds text with control characters removed: folder names and ids come
    /// from hook payloads and must not drive the terminal.
    fn add(mut self, style: &'static str, text: impl Into<String>) -> Self {
        let text = text.into().chars().filter(|c| !c.is_control()).collect();
        self.0.push((style, text));
        self
    }

    /// Writes the line, cut off at `width` columns.
    fn write(&self, width: usize, out: &mut String) {
        let mut room = width;
        for (style, text) in &self.0 {
            let shown: String = text.chars().take(room).collect();
            if shown.is_empty() {
                continue;
            }
            room -= shown.chars().count();
            if style.is_empty() {
                out.push_str(&shown);
            } else {
                out.push_str(style);
                out.push_str(&shown);
                out.push_str(RESET);
            }
        }
    }
}

fn now() -> Millis {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as Millis)
}

/// The terminal's width and height, or 80 by 24 when stdout isn't one.
fn terminal_size() -> (usize, usize) {
    // SAFETY: winsize is plain data, so all-zero bytes are valid.
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    // SAFETY: TIOCGWINSZ writes one winsize to the pointer it is given.
    let found = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) } == 0;
    if found && size.ws_col > 0 && size.ws_row > 0 {
        (usize::from(size.ws_col), usize::from(size.ws_row))
    } else {
        (80, 24)
    }
}
