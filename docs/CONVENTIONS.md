# Conventions

- Offsets in `core` are byte offsets into UTF-8 text, on a char boundary. A UI converts at its own edge.
- AI state lives in `core`. A UI renders it and never keeps a second copy.
- Tests assert what a user of the code can observe. Provider tests run against a local mock server.
- Headless proof for UI: each app has a script that renders screenshots without taking focus.
- Decisions go in `docs/adr/`, one per file, never edited after acceptance.
