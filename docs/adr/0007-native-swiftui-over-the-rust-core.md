# 0007. A native SwiftUI app over the Rust core, with reading at its centre

Status: Accepted, 2026-11-26. Supersedes ADR 0001 once the native app replaces the GPUI one.

## Context

On 2026-11-26 Toby asked for Inkling to "look and feel like a native Swift app, much like Matter,
with the same kinds of integrations Matter has. The goal is basically my own local copy of
Matter." Matter is a read-later app for iPhone, iPad, Mac and the web: a Queue of saved articles,
an Inbox of newsletters, followed writers and RSS feeds, a reader, text-to-speech that the reader
follows, highlights with notes, a Notebook, and highlights synced to Notion, Obsidian, Roam and
Readwise. Its iOS 26 build uses Liquid Glass and native components. The plan, with the research
and the sources, is on the Notion page "Plan: Inkling as my own Matter"
(https://app.notion.com/p/424935c51fac81a9b8d5cdc036cce9e8).

Inkling draws every screen with GPUI Kit on both devices (ADR 0001). That gives one Rust UI, but
GPUI draws its own controls, so the app only imitates the platform: no Liquid Glass, no system
sidebar or toolbar, its own selection and menus, and on iPhone autocorrect, suggestions and the
loupe need a patched platform crate, and VoiceOver needed a bridge of its own (TOB-531). Several
of Matter's features need native code whatever draws the screens: a share extension and a Safari
web extension are separate native targets, and background audio with lock-screen controls is
AVFoundation.

ADR 0001 already named a native iPhone screen over the same core as the fallback, and the stack
research of 2026-11-11 found a shared Rust core under a native UI, joined by UniFFI, to be the
common way to ship Rust on phones.

Three ways were put to Toby: a native SwiftUI app over the Rust core, GPUI restyled to look
native, or a native shell with GPUI views inside it. He chose the first.

## Decision

- **Swift owns every screen; Rust owns everything else.** One SwiftUI app, `apple/`, with a Mac
  target and an iPhone target. `inkling-core`, `inkling-books`, `inkling-sync` and `inkling-mcp`
  stay as they are: parsing, the library, documents, settings, sync, the providers and the review
  model. A new crate, `ffi/` (`inkling-ffi`), exposes them to Swift through UniFFI and is built
  into an XCFramework and a Swift package by a script.
- **Reading is the centre.** The app's sections are Queue, Inbox, Library, Notebook and Writing.
  Articles join books in the library as `Format::Article`, one chapter of Markdown each (ADR 0006),
  so highlights, places, Find, sync and the MCP tools work on them unchanged.
- **Native where it shows.** On the Mac a three-column `NavigationSplitView`, a unified toolbar,
  menus and keyboard commands, and a Settings window; on iPhone a Liquid Glass `TabView`, swipe
  actions and sheets. Text is drawn by TextKit 2. Listening uses AVSpeechSynthesizer and
  AVFoundation.
- **Inkling stays Inkling.** Its name, icon, Source Serif 4 and Inter, the paper tones and its
  palette. Matter is the reference for the feel and the features, not the look.
- **Writing moves last.** The live Markdown page and the inline review are rebuilt on TextKit 2
  (`NSTextView`, `UITextView`) after the reading screens. Until then the GPUI app keeps writing.
  Once it moves, `ui/`, `mac/` and the GPUI host in `ios/` are deleted.
- **The server still never parses.** On Cloudflare's free plan a Worker has 10 ms of CPU, so, as
  with books (ADR 0004), the Worker fetches and keeps pages, feeds and newsletters, and the devices
  parse them.

## Consequences

- The views in `ui/` are written again in Swift, the writing page hardest of all. Reading first
  keeps a usable app at every step.
- Swift for Apple's platforms builds only on a Mac, so the Swift work runs there (through Remote
  Control from the cloud), headless as `CLAUDE.md` requires. The Rust crates and the Worker still
  build and test anywhere.
- The proof for native screens moves from `mac/snap.sh` and `ios/snap.sh` to Xcode tests: snapshot
  renders on the Simulator and offscreen on the Mac. `CLAUDE.md` changes with the first native
  screen.
- Rust and Swift meet at one boundary, `inkling-ffi`, whose types are plain records and errors, so
  the crates under it keep no Swift or UniFFI dependency.
- The iPhone patches in `ios/patches/` and the GPUI accessibility bridge go when the GPUI host does.

## Alternatives rejected

- **Restyle GPUI Kit to look like macOS and iOS.** Cheapest, but it stays an imitation, and the
  share and Safari extensions and the audio still need Swift.
- **A native shell with GPUI views inside.** Two UI frameworks, two text stacks and two event
  loops in one window, which ADR 0003 turned down for Rune for the same reason.
