//! A local HTTP server for tests: serves one request with a canned reply and hands back what it
//! saw. The providers and feedback tests both post to it.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// What the mock server sends back.
pub enum Reply {
    /// A 200 event stream, one chunk per event. `hold` keeps the connection open after them.
    Stream { events: Vec<String>, hold: bool },
    Status { code: u16, body: String },
    /// A 302 to `location`.
    Redirect(String),
}

/// What the mock server saw: the request line, the headers (lowercased names) and the body.
pub struct Seen {
    pub line: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// Serves one request on a free local port. Returns its base URL and the request it saw.
pub fn serve(reply: Reply) -> (String, mpsc::Receiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        let seen = read_request(&socket);
        tx.send(seen).unwrap();
        let mut socket = socket;
        match reply {
            Reply::Status { code, body } => {
                let head = format!(
                    "HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).and_then(|_| socket.write_all(body.as_bytes()));
            }
            Reply::Redirect(location) => {
                let head = format!("HTTP/1.1 302 Found\r\nlocation: {location}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
                let _ = socket.write_all(head.as_bytes());
            }
            Reply::Stream { events, hold } => {
                let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n";
                let _ = socket.write_all(head.as_bytes());
                for event in events {
                    let _ = write!(socket, "{:x}\r\n{event}\r\n", event.len());
                    let _ = socket.flush();
                }
                if hold {
                    thread::sleep(Duration::from_secs(30));
                } else {
                    let _ = socket.write_all(b"0\r\n\r\n");
                }
            }
        }
    });
    (base, rx)
}

fn read_request(socket: &TcpStream) -> Seen {
    let mut reader = BufReader::new(socket);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':').unwrap();
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    let length: usize = headers.iter().find(|(n, _)| n == "content-length").map_or(0, |(_, v)| v.parse().unwrap());
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    Seen { line: line.trim_end().into(), headers, body: String::from_utf8(body).unwrap() }
}
