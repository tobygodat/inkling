//! `inkling-mcp --stdio`: Inkling's MCP server for an agent on this Mac, such as Claude Desktop or
//! Claude Code, which starts it and talks to it over stdin and stdout. It reads the app's data
//! folder (`INKLING_DATA_DIR` when set) and writes nothing.

use inkling_mcp::{Folder, Server};
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

const USAGE: &str = "Usage: inkling-mcp --stdio

Serves Inkling's library and draft to an agent over stdio. Add it to Claude Code with:
  claude mcp add inkling -- inkling-mcp --stdio";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args != ["--stdio"] {
        eprintln!("{USAGE}");
        return if args.iter().any(|a| a == "--help" || a == "-h") { ExitCode::SUCCESS } else { ExitCode::from(2) };
    }
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
