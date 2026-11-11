# Architecture

## Crates
- `core/` is `inkling-core`. It has no UI dependency. It holds the AI request and the three providers,
  the proposal and its accept or dismiss, settings, and key storage.
- `apps/gpui-kit/` and `apps/gpui-kit-ios-spike/` are the two prototypes kept as seeds for the real
  app. They are throwaway and go away as the real app replaces them.

## Data flow
A UI builds an `AiRequest`, picks a `Backend` with `backend_for`, and calls `start`. Events arrive on
a background thread through the sink. The UI forwards them to `Assist`, which is the only AI state.

## State
- The draft and settings are files under the app's data folder (`Paths`).
- API keys are in the Apple Keychain (`KeychainKeys`), never in a file.

## External services
Anthropic, OpenAI and Google's generative language API, each called directly with the user's key.
