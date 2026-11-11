# Inkling

A minimal writing app for Mac and iPhone, in Rust, with bring-your-own-key AI (Claude, Gemini, ChatGPT).
"Inkling" is a working name.

The UI is GPUI Kit on both devices (see `docs/adr/0001-gpui-kit-on-mac-and-iphone.md`) and the design
is in `DESIGN.md`. Today the repo holds the shared core and two prototypes kept as seeds.

## Run

```
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
cargo test --manifest-path core/Cargo.toml
```

`apps/gpui-kit` is the Mac prototype and `apps/gpui-kit-ios-spike` the iPhone one. Each has a README
with its run and screenshot commands.
