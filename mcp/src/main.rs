//! `inkling-mcp --stdio`: Inkling's MCP server for an agent on this Mac, such as Claude Desktop or
//! Claude Code, which starts it and talks to it over stdin and stdout. It reads the app's data
//! folder (`INKLING_DATA_DIR` when set) and writes nothing.
//!
//! `sign-in`, `sync`, `readwise` and `sign-out` put the same library on Inkling's server, for an
//! agent away from this Mac (ADR 0004).

use inkling_core::store::{KeyStore, Secret, default_keys};
use inkling_mcp::signin::{self, DEFAULT_SERVER, Http, SignIn};
use inkling_mcp::sync::{self, Body, Error, Method, Remote};
use inkling_mcp::{Folder, Server};
use serde_json::{Value, json};
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

const USAGE: &str = "Usage:
  inkling-mcp --stdio            Serve the library and draft to an agent on this Mac.
  inkling-mcp sign-in [server]   Sign this Mac in to Inkling's server, in the browser.
  inkling-mcp sync               Send the library and draft up, and fetch Readwise uploads
                                 into ~/Downloads/Readwise for Find books.
  inkling-mcp readwise           Connect Readwise to the server.
  inkling-mcp sign-out           Forget this Mac's sign-in.

Add the local server to Claude Code with:
  claude mcp add inkling -- inkling-mcp --stdio";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let done = match args.as_slice() {
        ["--stdio"] => return stdio(),
        ["sign-in"] => sign_in(DEFAULT_SERVER),
        ["sign-in", server] => sign_in(server),
        ["sync"] => run_sync(),
        ["readwise"] => readwise(),
        ["sign-out"] => sign_out(),
        _ => {
            eprintln!("{USAGE}");
            return if args.iter().any(|a| *a == "--help" || *a == "-h") { ExitCode::SUCCESS } else { ExitCode::from(2) };
        }
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
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

fn sign_in(server: &str) -> Result<(), String> {
    let keys = default_keys();
    let signed = signin::sign_in(server, |address| {
        println!("Opening your browser to sign in. If it doesn't open, go to:\n{address}");
        if cfg!(target_os = "macos") {
            let _ = Command::new("open").arg(address).status();
        }
    })
    .map_err(|e| e.to_string())?;
    signed.save(keys.as_ref())?;
    println!("Signed in to {}. Run `inkling-mcp sync` to send your library up.", signed.server);
    Ok(())
}

/// The sync API with a fresh token, keeping any new refresh token the server handed back.
fn connect(keys: &dyn KeyStore) -> Result<Http, Error> {
    let mut signed = SignIn::load(keys).ok_or(Error::SignedOut)?;
    let token = signed.refresh()?;
    signed.save(keys).map_err(Error::Local)?;
    Ok(Http::new(&signed.server, token))
}

fn run_sync() -> Result<(), String> {
    let keys = default_keys();
    let remote = connect(keys.as_ref()).map_err(|e| e.to_string())?;
    let folder = Folder::mac();
    let inbox = home().join("Downloads").join("Readwise");
    let report = sync::sync(&folder, &remote, &inbox).map_err(|e| e.to_string())?;

    for title in &report.uploaded {
        println!("Sent up: {title}");
    }
    for (title, why) in &report.skipped {
        println!("Not sent: {title}, because {why}.");
    }
    if report.positions > 0 {
        println!("Reading positions sent: {}", report.positions);
    }
    if report.draft {
        println!("Draft sent.");
    }
    for path in &report.fetched {
        println!("From Readwise: {} (add it with Find books)", path.display());
    }
    if report == sync::Report::default() {
        println!("Already up to date.");
    }
    Ok(())
}

fn readwise() -> Result<(), String> {
    let keys = default_keys();
    let remote = connect(keys.as_ref()).map_err(|e| e.to_string())?;
    let status = remote.json(Method::Get, "/v1/readwise", Body::None).map_err(|e| e.to_string())?;
    let hook = status.get("webhook_url").and_then(Value::as_str).unwrap_or_default().to_string();

    let token = ask("Readwise access token, from readwise.io/access_token: ")?;
    if token.is_empty() {
        return Err("No token, so nothing changed.".into());
    }
    println!(
        "\nFor uploads to arrive within a minute rather than half an hour, add a webhook at\n\
         readwise.io/webhook for \"Reader: new non-feed document\" with the URL\n  {hook}\n"
    );
    let secret = ask("Its secret (or press Return to skip): ")?;
    let body = json!({ "token": token, "webhook_secret": if secret.is_empty() { Value::Null } else { Value::String(secret) } });
    let status = remote.json(Method::Put, "/v1/readwise", Body::Json(body)).map_err(|e| e.to_string())?;
    let webhook = status.get("webhook").and_then(Value::as_bool).unwrap_or(false);
    println!(
        "Readwise is connected{}. EPUBs and PDFs you save to Reader from now on reach this Mac at the next sync.",
        if webhook { " with its webhook" } else { "; the server checks it every 30 minutes" }
    );
    Ok(())
}

fn sign_out() -> Result<(), String> {
    let keys = default_keys();
    keys.remove(Secret::ServerSignIn)?;
    println!("Signed out on this Mac. The server keeps what was synced.");
    Ok(())
}

/// Reads one line, without echoing it when typed at a terminal, since it is a secret.
fn ask(prompt: &str) -> Result<String, String> {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let hidden = io::stdin().is_terminal() && Command::new("stty").arg("-echo").status().is_ok_and(|s| s.success());
    let mut line = String::new();
    let read = io::stdin().lock().read_line(&mut line);
    if hidden {
        let _ = Command::new("stty").arg("echo").status();
        println!();
    }
    read.map_err(|e| e.to_string())?;
    Ok(line.trim().to_string())
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}
