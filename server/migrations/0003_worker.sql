-- The Worker phase (ADR 0007, TOB-554): synced articles, incoming HTML pages (saved URLs, feed
-- items and newsletters waiting for a device to parse with inkling-books), RSS/Atom feeds and
-- highlights exported once each to Readwise.

-- Articles join books in the library (`Format::Article`), carrying where they came from and whether
-- they landed in the Inbox (feeds and newsletters) rather than the Queue.
CREATE TABLE books_next (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  author TEXT,
  format TEXT NOT NULL CHECK (format IN ('Epub', 'Pdf', 'Article')),
  chapters INTEGER NOT NULL,
  cover TEXT,
  added INTEGER NOT NULL,
  chapter INTEGER NOT NULL DEFAULT 0,
  fraction REAL NOT NULL DEFAULT 0,
  position_at INTEGER NOT NULL DEFAULT 0,
  source_url TEXT,
  inbox INTEGER NOT NULL DEFAULT 0
);

INSERT INTO books_next (id, title, author, format, chapters, cover, added, chapter, fraction, position_at)
  SELECT id, title, author, format, chapters, cover, added, chapter, fraction, position_at FROM books;

DROP TABLE books;
ALTER TABLE books_next RENAME TO books;

-- Raw HTML articles waiting for a device to parse with inkling-books (ADR 0007): saved URLs go to
-- the Queue, feed items and newsletters go to the Inbox. A waiting item's HTML lives in FILES at
-- `incoming/<id>` until a device settles it.
CREATE TABLE incoming (
  id TEXT PRIMARY KEY,
  source TEXT NOT NULL CHECK (source IN ('save', 'feed', 'newsletter')),
  destination TEXT NOT NULL CHECK (destination IN ('queue', 'inbox')),
  url TEXT NOT NULL,
  title TEXT,
  author TEXT,
  site TEXT,
  published TEXT,
  feed_id TEXT,
  dedup_key TEXT NOT NULL UNIQUE,
  state TEXT NOT NULL CHECK (state IN ('waiting', 'imported', 'too_large', 'failed')),
  bytes INTEGER,
  error TEXT,
  book_id TEXT,
  received_at INTEGER NOT NULL
);

CREATE INDEX incoming_state ON incoming (state, received_at DESC);

-- Followed RSS and Atom feeds, checked by the Worker cron.
CREATE TABLE feeds (
  id TEXT PRIMARY KEY,
  url TEXT NOT NULL UNIQUE,
  title TEXT NOT NULL,
  site_url TEXT,
  etag TEXT,
  last_modified TEXT,
  added_at INTEGER NOT NULL,
  checked_at INTEGER NOT NULL DEFAULT 0,
  last_error TEXT
);

-- Items already seen in a feed, so a cron poll never queues the same item twice even after the
-- incoming row is settled.
CREATE TABLE feed_seen (
  feed_id TEXT NOT NULL,
  item_key TEXT NOT NULL,
  seen_at INTEGER NOT NULL,
  PRIMARY KEY (feed_id, item_key)
);

-- Passages highlighted on any device. `id` is the SHA-256 of the book, chapter, byte range and
-- words, so syncing from several devices never creates duplicates. `readwise_sent_at` is set once
-- Readwise accepts the highlight; `readwise_sending` claims rows while a send is in flight.
CREATE TABLE highlights (
  id TEXT PRIMARY KEY,
  book_id TEXT NOT NULL,
  chapter INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  words TEXT NOT NULL,
  note TEXT,
  made INTEGER NOT NULL,
  readwise_sent_at INTEGER,
  readwise_sending INTEGER
);

CREATE INDEX highlights_book ON highlights (book_id, chapter, start_byte, made);
CREATE INDEX highlights_unsent ON highlights (readwise_sent_at) WHERE readwise_sent_at IS NULL;
