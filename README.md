# Inkling

A minimal writing app for Mac and iPhone, in Rust, with bring-your-own-key AI (Claude, Gemini, ChatGPT).
"Inkling" is a working name.

The repo is at the prototype stage: one shared core and the same small writing slice built on three UI
stacks, to choose between them. See `docs/prototype-slice.md`.

## Run

```
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
cargo test --manifest-path core/Cargo.toml
```

Each prototype under `apps/` has its own README with its run and screenshot commands.
