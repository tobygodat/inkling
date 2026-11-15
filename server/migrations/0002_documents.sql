-- The writer's documents, as the devices sync them (ADR 0004, TOB-490). The id is the device's
-- file name. `version` counts up on every write the server takes, and a write names the version
-- it was made on, so no save replaces one it hasn't seen. A deleted document keeps its last text.
CREATE TABLE documents (
  id TEXT PRIMARY KEY,
  text TEXT NOT NULL,
  saved_at INTEGER NOT NULL,
  device TEXT NOT NULL,
  version INTEGER NOT NULL,
  deleted INTEGER NOT NULL DEFAULT 0
);

-- The one draft becomes document `draft`, the id the devices gave draft.md when documents came in.
INSERT INTO documents (id, text, saved_at, device, version)
  SELECT 'draft', text, saved_at, 'server', 1 FROM drafts ORDER BY version DESC LIMIT 1;
DROP TABLE drafts;

-- Readwise arrives by the 30-minute poll alone.
ALTER TABLE readwise DROP COLUMN webhook_secret;
