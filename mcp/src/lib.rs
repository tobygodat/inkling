//! Inkling's MCP server: the tools an agent such as Claude uses to read the writer's library and
//! draft (ADR 0004). [`Server`] answers one JSON-RPC message at a time and knows nothing about
//! the transport; `main.rs` serves it over stdio. Every tool only reads, through a [`Store`], so
//! the same tools run on the server over a database.

mod store;
mod tools;

use serde_json::{Value, json};

pub use store::{Folder, Store};

/// The protocol versions this server speaks, newest first. A client asking for another one gets
/// the newest, as the spec says, and decides for itself whether to go on.
pub const VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Inkling is the writer's app for writing and reading. These tools read \
their library of books (EPUBs and PDFs) and their draft. They never change anything. Chapters \
and positions count from 0.";

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

pub struct Server<S> {
    store: S,
}

impl<S: Store> Server<S> {
    pub fn new(store: S) -> Self {
        Server { store }
    }

    /// Answers one line of input. A notification, or a response the client sent us, gets none.
    pub fn handle_line(&self, line: &str) -> Option<String> {
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(message) => self.handle(&message)?,
            Err(e) => failure(Value::Null, PARSE_ERROR, &format!("Not JSON: {e}")),
        };
        Some(reply.to_string())
    }

    pub fn handle(&self, message: &Value) -> Option<Value> {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // A response to a request we never send, or junk: a response needs an id to go to.
            return match message.get("id") {
                Some(id) if message.get("result").is_none() && message.get("error").is_none() => {
                    Some(failure(id.clone(), INVALID_REQUEST, "A request needs a method."))
                }
                _ => None,
            };
        };
        let id = message.get("id")?.clone();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        Some(match method {
            "initialize" => success(id, initialize(&params)),
            "ping" => success(id, json!({})),
            "tools/list" => success(id, json!({ "tools": tools::list() })),
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Value::as_str) else {
                    return Some(failure(id, INVALID_PARAMS, "tools/call needs a tool name."));
                };
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                match tools::call(&self.store, name, &arguments) {
                    Some(result) => success(id, result),
                    None => failure(id, INVALID_PARAMS, &format!("There is no tool called {name}.")),
                }
            }
            _ => failure(id, METHOD_NOT_FOUND, &format!("{method} isn't supported.")),
        })
    }
}

fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = asked.filter(|v| VERSIONS.contains(v)).unwrap_or(VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "inkling", "title": "Inkling", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn failure(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkling_books::{Book, BookId, Chapter, Format, Library, PARSER, Position};

    fn library() -> (tempfile::TempDir, Server<Folder>) {
        let dir = tempfile::tempdir().unwrap();
        let mut library = Library::open(dir.path().join("library")).unwrap();
        let book = Book {
            id: BookId("ab".repeat(32)),
            title: "The Waves".into(),
            author: Some("Virginia Woolf".into()),
            format: Format::Epub,
            parser: PARSER,
            chapters: vec![
                Chapter { title: Some("One".into()), text: "# One\n\nThe sun had not yet risen.\n".into() },
                Chapter {
                    title: Some("Two".into()),
                    text: "# Two\n\n## Morning\n\nThe sea was *indistinguishable* from the sky.\n\n> I see a ring,\\\n> said Bernard.\n".into(),
                },
            ],
            source: None,
        };
        library.insert(&book).unwrap();
        library.set_position(&book.id, Position { chapter: 1, fraction: 0.5 }).unwrap();
        std::fs::write(dir.path().join("draft.md"), "# A draft\n\nIt begins.").unwrap();
        let server = Server::new(Folder::new(dir.path()));
        (dir, server)
    }

    fn call(server: &Server<Folder>, name: &str, arguments: Value) -> Value {
        let request = json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call",
            "params": { "name": name, "arguments": arguments } });
        let reply = server.handle(&request).unwrap();
        assert_eq!(reply["id"], 7);
        reply["result"].clone()
    }

    fn text(result: &Value) -> &str {
        result["content"][0]["text"].as_str().unwrap()
    }

    #[test]
    fn the_handshake_agrees_a_version_and_notifications_get_no_reply() {
        let (_dir, server) = library();
        let ask = |version: &str| {
            let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "protocolVersion": version, "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } } });
            server.handle(&request).unwrap()["result"]["protocolVersion"].clone()
        };
        assert_eq!(ask("2025-06-18"), "2025-06-18");
        assert_eq!(ask("1999-01-01"), VERSIONS[0]);
        assert!(server.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        assert!(server.handle(&json!({ "jsonrpc": "2.0", "id": 3, "result": {} })).is_none());

        let unknown = server.handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/list" })).unwrap();
        assert_eq!(unknown["error"]["code"], METHOD_NOT_FOUND);
        let junk: Value = serde_json::from_str(&server.handle_line("{not json").unwrap()).unwrap();
        assert_eq!(junk["error"]["code"], PARSE_ERROR);
        assert_eq!(junk["id"], Value::Null);

        let listed = server.handle(&json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list" })).unwrap();
        let names: Vec<&str> =
            listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["list_books", "read_book", "search_library", "read_draft", "list_documents", "read_document", "list_highlights"]);
        let missing = server.handle(&json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": { "name": "delete_everything" } })).unwrap();
        assert_eq!(missing["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn the_tools_read_the_library_and_the_draft_from_the_data_folder() {
        let (dir, server) = library();
        let id = "ab".repeat(32);

        let books: Value = serde_json::from_str(text(&call(&server, "list_books", json!({})))).unwrap();
        assert_eq!(books[0]["id"], id.as_str());
        assert_eq!(books[0]["title"], "The Waves");
        assert_eq!(books[0]["position"]["chapter"], 1);
        assert_eq!(books[0]["progress"], 0.75);

        let contents = text(&call(&server, "read_book", json!({ "book_id": id }))).to_string();
        assert!(contents.contains("0. One") && contents.contains("1. Two"), "{contents}");

        let chapter = call(&server, "read_book", json!({ "book_id": id, "chapter": 1 }));
        assert_eq!(
            text(&chapter),
            "# Two\n\n## Morning\n\nThe sea was *indistinguishable* from the sky.\n\n> I see a ring,\\\n> said Bernard.\n",
            "the chapter's Markdown as the library stores it"
        );
        let past = call(&server, "read_book", json!({ "book_id": id, "chapter": 2 }));
        assert_eq!(past["isError"], true);
        let unknown = call(&server, "read_book", json!({ "book_id": "nope" }));
        assert_eq!(unknown["isError"], true);

        let hits: Value = serde_json::from_str(text(&call(&server, "search_library", json!({ "query": "THE SEA" })))).unwrap();
        assert_eq!(hits.as_array().unwrap().len(), 1);
        assert_eq!(hits[0]["chapter"], 1);

        let library = Library::open(dir.path().join("library")).unwrap();
        let later = inkling_books::Highlight { chapter: 1, start: 26, end: 51, words: "indistinguishable".into(), made: 1_700_000_000_000 };
        let earlier = inkling_books::Highlight { chapter: 0, start: 7, end: 14, words: "The sun".into(), made: 1_700_000_100_000 };
        library.set_highlights(&BookId(id.clone()), &[later, earlier]).unwrap();
        for arguments in [json!({}), json!({ "book_id": id })] {
            let listed: Value = serde_json::from_str(text(&call(&server, "list_highlights", arguments))).unwrap();
            let words: Vec<&str> = listed.as_array().unwrap().iter().map(|h| h["words"].as_str().unwrap()).collect();
            assert_eq!(words, ["The sun", "indistinguishable"], "in reading order");
            assert_eq!((listed[1]["book_id"].as_str(), listed[1]["chapter"].as_u64(), listed[1]["start"].as_u64()), (Some(id.as_str()), Some(1), Some(26)));
        }
        assert_eq!(call(&server, "list_highlights", json!({ "book_id": "nope" }))["isError"], true);

        // Before the app has moved draft.md into documents/, the draft is draft.md.
        assert_eq!(text(&call(&server, "read_draft", json!({}))), "# A draft\n\nIt begins.");
    }

    #[test]
    fn the_documents_are_listed_and_read_and_the_draft_is_the_one_open_last() {
        let (dir, server) = library();
        let paths = inkling_core::Paths { dir: dir.path().to_path_buf() };
        let documents = inkling_core::Documents::new(&paths);
        documents.migrate(&paths).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        documents.restore("2", "# Second\n\nNewer.", later).unwrap();
        let listed: Value = serde_json::from_str(text(&call(&server, "list_documents", json!({})))).unwrap();
        let titles: Vec<&str> = listed.as_array().unwrap().iter().map(|d| d["title"].as_str().unwrap()).collect();
        assert_eq!(titles, ["Second", "A draft"]);
        assert_eq!(text(&call(&server, "read_document", json!({ "document_id": "draft" }))), "# A draft\n\nIt begins.");
        assert_eq!(call(&server, "read_document", json!({ "document_id": "../settings" }))["isError"], true);
        assert_eq!(text(&call(&server, "read_draft", json!({}))), "# Second\n\nNewer.");
        let mut settings = inkling_core::Settings::load(&paths);
        settings.document = Some("draft".into());
        settings.save(&paths).unwrap();
        assert_eq!(text(&call(&server, "read_draft", json!({}))), "# A draft\n\nIt begins.");
    }
}
