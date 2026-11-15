//! `inkling-mcp --stdio`: Inkling's MCP server for an agent on this Mac, such as Claude Desktop or
//! Claude Code, which starts it and talks to it over stdin and stdout. It reads the app's data
//! folder (`INKLING_DATA_DIR` when set) and writes nothing. The app itself keeps the server in
//! step, for agents away from this Mac (ADR 0004).

use inkling_mcp::{Folder, Server};
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

const USAGE: &str = "Usage:
  inkling-mcp --stdio            Serve the library and documents to an agent on this Mac.

Add it to Claude Code with:
  claude mcp add inkling -- inkling-mcp --stdio";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["--stdio"] => stdio(),
        other => {
            eprintln!("{USAGE}");
            if other.iter().any(|a| *a == "--help" || *a == "-h") { ExitCode::SUCCESS } else { ExitCode::from(2) }
        }
    }
}

fn stdio() -> ExitCode {
    let folder = Folder::mac();
    eprintln!("inkling-mcp: reading {}", folder.dir().display());
    let server = Server::new(folder);

    let stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    for line in stdin.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle_line(&line) {
            // The client has gone if this fails; there is no one left to tell.
            if writeln!(stdout, "{reply}").and_then(|()| stdout.flush()).is_err() {
                break;
            }
        }
    }
    ExitCode::SUCCESS
}
