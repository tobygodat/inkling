-- Inkling has one writer, so no table carries a user id. Only the Apple ID in OWNER_APPLE_SUB can
-- sign in (src/authorize.ts).

-- A book a device imported. The id is the SHA-256 of its file, as on the device, so a book can't
-- be added twice. The row is written last, after its parsed file and chapters, so a row always has
-- a book behind it.
CREATE TABLE books (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  author TEXT,
  format TEXT NOT NULL CHECK (format IN ('Epub', 'Pdf')),
  chapters INTEGER NOT NULL,
  cover TEXT,
  added INTEGER NOT NULL,
  chapter INTEGER NOT NULL DEFAULT 0,
  fraction REAL NOT NULL DEFAULT 0,
  position_at INTEGER NOT NULL DEFAULT 0
);

-- Each chapter as Markdown, exactly as `read_book` gives it, and its title for the contents.
CREATE TABLE chapters (
  book_id TEXT NOT NULL,
  n INTEGER NOT NULL,
  title TEXT NOT NULL,
  text TEXT NOT NULL,
  PRIMARY KEY (book_id, n)
);

-- Trigrams, so search finds any piece of text of three or more characters in any case, as the
-- Mac's search_library finds any piece. The search runs inside D1, not in the Worker's CPU time.
CREATE VIRTUAL TABLE chapters_search USING fts5(text, content = 'chapters', content_rowid = 'rowid', tokenize = 'trigram');
CREATE TRIGGER chapters_added AFTER INSERT ON chapters BEGIN
  INSERT INTO chapters_search (rowid, text) VALUES (new.rowid, new.text);
END;
CREATE TRIGGER chapters_removed AFTER DELETE ON chapters BEGIN
  INSERT INTO chapters_search (chapters_search, rowid, text) VALUES ('delete', old.rowid, old.text);
END;
CREATE TRIGGER chapters_changed AFTER UPDATE ON chapters BEGIN
  INSERT INTO chapters_search (chapters_search, rowid, text) VALUES ('delete', old.rowid, old.text);
  INSERT INTO chapters_search (rowid, text) VALUES (new.rowid, new.text);
END;

-- The draft as each save left it. The newest is the draft; the 19 before it are kept so a lost
-- edit can be got back.
CREATE TABLE drafts (
  version INTEGER PRIMARY KEY AUTOINCREMENT,
  text TEXT NOT NULL,
  saved_at INTEGER NOT NULL
);

-- The Readwise link: one row. The token and the webhook secret are AES-GCM ciphertext under
-- SECRET_KEY. Only documents saved after `connected_at` come in; `since` is how far the backstop
-- poll has read.
CREATE TABLE readwise (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  token TEXT NOT NULL,
  webhook_secret TEXT,
  connected_at TEXT NOT NULL,
  since TEXT NOT NULL
);

-- EPUBs and PDFs from Readwise, waiting for a device to import them. A waiting item's file is in
-- FILES at inbox/<readwise_id> until a device says how its import went.
CREATE TABLE inbox (
  readwise_id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  author TEXT,
  category TEXT NOT NULL CHECK (category IN ('epub', 'pdf')),
  state TEXT NOT NULL CHECK (state IN ('waiting', 'imported', 'too_large', 'failed')),
  bytes INTEGER,
  error TEXT,
  book_id TEXT,
  received_at INTEGER NOT NULL
);
