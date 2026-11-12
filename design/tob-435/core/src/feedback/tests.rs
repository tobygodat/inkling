//! SKETCH: core/src/feedback/tests.rs — the behaviours that can break, one test each, at the
//! lowest layer. Properties where the input space is wide (CLAUDE.md "Tests").
//!
//! Left out on purpose: status derivation beyond what `records_decide_status` shows (it is a map
//! lookup), Secret::account (a match the compiler checks), the fake transport (the snaps drive it),
//! RFC 3339 formatting past the one case in the body test.

/// Key handling and redaction. Property over valid secrets and URLs:
/// - `FeedbackLink::parse(link.to_stored()) == link`
/// - `format!("{link:?}")`, `link.url()` and every `BadLink` message never contain the secret
/// - http is refused for any host but 127.0.0.1 / localhost; no `#k=`, an empty or a short secret
///   is refused; surrounding whitespace is trimmed.
#[test]
fn a_link_round_trips_and_never_shows_its_secret() {
    todo!()
}

/// The signature T3 verifies. RFC 4231 test case 2 (key "Jefe", data "what do ya want for
/// nothing?") gives `sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843`.
#[test]
fn the_signature_is_hmac_sha256_hex() {
    todo!()
}

/// PRODUCT.md: writing goes only to the provider the writer picked. The body's JSON keys are
/// exactly id, text, sent_at, form_factor, window{width,height}, screen, version, build. If you
/// are here because you added a field: the body must never carry draft text or a key. Change this
/// list only if the new field is neither.
#[test]
fn the_body_holds_only_the_agreed_fields() {
    // Desk with a memory store holding a link AND an API key, `send("Bigger margins", …)`, then
    // decode the Post's body: assert the key set, sent_at == rfc3339(id.sent_at()), and that the
    // API key's text appears nowhere in the body or the url.
    todo!()
}

/// T3 dedupes on the id. Property over pairs of texts: `Attempt::next(Some(&a), &a.text, _)`
/// keeps `a.id`; any other text gets a fresh id.
#[test]
fn a_retry_keeps_its_id_only_for_the_same_text() {
    todo!()
}

/// End to end through ureq, against the mock server in providers/tests.rs (moved to a shared
/// `#[cfg(test)] mod mock` so both use it — extend before you add):
/// - 202 → Ok; the request is a POST to the link's path, the body bytes are exactly the signed
///   bytes, the signature header verifies, and the secret appears nowhere in the request.
/// - 401 → Refused, 500 → Server(500), a closed port → Offline.
/// Then `settle`: Ok appends to the log once, even if called for the same id twice; Err keeps the
/// attempt and the log unchanged.
#[test]
fn a_send_posts_signed_json_and_maps_failures() {
    todo!()
}

/// Persistence. Extends store.rs's `draft_and_settings_survive_a_reload` pattern: two sends,
/// reopen a Desk on the same Paths, `items()` lists both newest first; a log with a duplicate id
/// and one garbage entry loads the rest.
#[test]
fn the_sent_log_survives_a_reload() {
    todo!()
}

/// The record format, through the one parser the build script also uses:
/// - `parse(to_json(r)) == r` for done and declined
/// - refused: id ≠ file name, unknown outcome, unknown field, summary on a declined record,
///   a line break, an empty line, a line over MAX_LINE
/// - `Build::with_records([done a, declined b])`: items for a, b, c are Done, Declined, Pending.
#[test]
fn records_decide_status() {
    todo!()
}
