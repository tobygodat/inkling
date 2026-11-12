use super::link::{BadLink, FeedbackLink, sign};
use super::*;
use crate::mock::{Reply, serve};
use crate::{MemoryKeys, Provider};
use proptest::prelude::*;
use std::net::TcpListener;
use std::time::Duration;

const SECRET: &str = "s3cr3t-0123456789abcdefABCDEF_xyz";

fn temp(name: &str) -> Paths {
    let dir = std::env::temp_dir().join(format!("inkling-feedback-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Paths { dir }
}

fn from() -> SentFrom {
    SentFrom { form_factor: "mac", window: (1280, 800), screen: "page" }
}

fn keys_with(link: &str) -> MemoryKeys {
    let keys = MemoryKeys::default();
    Desk::save_link(&keys, link).expect("a good link saves");
    keys
}

fn id(text: &str) -> FeedbackId {
    FeedbackId::parse(text).unwrap()
}

proptest! {
    /// Key handling and redaction: a stored link reads back as itself, and nothing a caller can
    /// print or post holds the secret.
    #[test]
    fn a_link_round_trips_and_never_shows_its_secret(
        secret in "[A-Za-z0-9_-]{32,64}",
        host in "[a-z0-9]{1,12}(\\.[a-z]{2,6}){0,2}(:[0-9]{2,5})?",
        path in "(/[A-Za-z0-9._~-]{0,12}){0,3}",
        pad in "[ \t\n]{0,2}",
    ) {
        let pasted = format!("{pad}https://{host}{path}#k={secret}{pad}");
        let link = FeedbackLink::parse(&pasted).unwrap();
        prop_assert_eq!(FeedbackLink::parse(&link.to_stored()), Ok(link.clone()));
        prop_assert_eq!(link.url(), format!("https://{host}{path}"));
        let shown = format!("{link:?}");
        prop_assert!(!shown.contains(&secret));

        let refused = [
            (format!("http://{host}{path}#k={secret}"), BadLink::NotHttps),
            (format!("https://{host}{path}"), BadLink::NoSecret),
            (format!("https://{host}{path}#k="), BadLink::NoSecret),
            (format!("https://{host}{path}#{secret}"), BadLink::NoSecret),
            (format!("https://{host}{path}#k={}", &secret[..31]), BadLink::ShortSecret),
            (format!("https://{host}{path}#k={secret}!"), BadLink::BadSecret),
            (format!("https://{host} {path}#k={secret}"), BadLink::NotAUrl),
            (format!("{host}{path}#k={secret}"), BadLink::NotAUrl),
        ];
        for (bad, why) in refused {
            prop_assert_eq!(FeedbackLink::parse(&bad), Err(why));
            prop_assert!(!why.to_string().contains(&secret));
        }
    }

    /// The webhook dedupes on the id: the same text retried keeps it, other text gets a new one.
    #[test]
    fn a_retry_keeps_its_id_only_for_the_same_text(first in ".{0,40}", second in ".{0,40}", at in 0u64..4_000_000_000) {
        let now = UNIX_EPOCH + Duration::from_secs(at);
        let a = Attempt::next(None, &first, now);
        prop_assert_eq!(a.id.sent_at(), at);
        prop_assert_eq!(Attempt::next(Some(&a), &first, now + Duration::from_secs(60)), a.clone());
        let b = Attempt::next(Some(&a), &second, now);
        prop_assert_eq!(b.id == a.id, first == second);
        prop_assert_eq!(FeedbackId::parse(&b.id.to_string()), Ok(b.id));
    }
}

/// The signature the webhook verifies: RFC 4231 test case 2.
#[test]
fn the_signature_is_hmac_sha256_hex() {
    assert_eq!(
        sign(b"Jefe", b"what do ya want for nothing?"),
        "sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

/// PRODUCT.md: writing goes only to the provider the writer picked. If you are here because you
/// added a field: the body must never carry draft text or a key. Change this list only if the new
/// field is neither.
#[test]
fn the_body_holds_only_the_agreed_fields() {
    let keys = keys_with(&format!("https://hooks.example/inkling#k={SECRET}"));
    keys.set(Secret::ApiKey(Provider::Claude), "sk-ant-never-in-feedback").unwrap();
    let mut desk = Desk::open(temp("body"), Build::with_records(Some(BuildNumber(41)), []));
    let post = desk.send("  Bigger margins\n", &keys, from()).unwrap();
    let body: serde_json::Value = serde_json::from_slice(post.body()).unwrap();
    let mut fields: Vec<&str> = body.as_object().unwrap().keys().map(String::as_str).collect();
    fields.sort();
    assert_eq!(fields, ["build", "form_factor", "id", "screen", "sent_at", "text", "version", "window"]);
    assert_eq!(body["window"], serde_json::json!({ "width": 1280, "height": 800 }));
    assert_eq!((body["text"].as_str(), body["build"].as_u64()), (Some("Bigger margins"), Some(41)));
    let sent_at = id(body["id"].as_str().unwrap()).sent_at();
    assert_eq!(body["sent_at"].as_str(), Some(post::rfc3339(sent_at).as_str()));
    assert_eq!(post::rfc3339(1_762_899_330), "2025-11-11T22:15:30Z");
    for leak in ["sk-ant-never-in-feedback", SECRET] {
        assert!(!String::from_utf8_lossy(post.body()).contains(leak) && !post.url().contains(leak));
    }
}

/// End to end through the HTTP client against a local server, then `settle`.
#[test]
fn a_send_posts_signed_json_and_maps_failures() {
    let paths = temp("send");
    let mut desk = Desk::open(paths.clone(), Build::with_records(None, []));
    desk.fake = false;
    let send = |desk: &mut Desk, reply: Reply, text: &str| {
        let (base, seen) = serve(reply);
        let keys = keys_with(&format!("{base}/hooks/inkling?x=1#k={SECRET}"));
        let post = desk.send(text, &keys, from()).expect("a post");
        (post, seen)
    };

    let (post, seen) = send(&mut desk, Reply::Status { code: 202, body: "{}".into() }, "Bigger margins");
    let body = post.body().to_vec();
    assert_eq!(post.run(), Ok(()));
    let seen = seen.recv().unwrap();
    assert_eq!(seen.line, "POST /hooks/inkling?x=1 HTTP/1.1");
    assert_eq!(seen.body.as_bytes(), body);
    assert_eq!(seen.header("x-inkling-signature"), Some(sign(SECRET.as_bytes(), &body).as_str()));
    assert_eq!(seen.header("content-type"), Some("application/json"));
    assert!(!seen.line.contains(SECRET) && !seen.body.contains(SECRET));
    assert!(seen.headers.iter().all(|(_, v)| !v.contains(SECRET)));
    desk.settle(Ok(())).unwrap();
    desk.settle(Ok(())).unwrap();
    assert_eq!((desk.items().count(), desk.delivery()), (1, &Delivery::Idle));

    let (follow, followed) = serve(Reply::Status { code: 200, body: "{}".into() });
    for (reply, error) in [
        (Reply::Status { code: 401, body: String::new() }, SendError::Refused),
        (Reply::Status { code: 500, body: String::new() }, SendError::Server(500)),
        (Reply::Redirect(format!("{follow}/elsewhere")), SendError::Server(302)),
    ] {
        let (post, _seen) = send(&mut desk, reply, "Bigger title");
        let result = post.run();
        assert_eq!(result, Err(error.clone()));
        desk.settle(result).unwrap();
        assert_eq!(desk.delivery(), &Delivery::Failed(error));
    }
    assert!(followed.recv_timeout(Duration::from_millis(200)).is_err(), "a redirect is never followed");

    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let keys = keys_with(&format!("http://127.0.0.1:{port}/hooks#k={SECRET}"));
    let failed = desk.attempt.clone().unwrap();
    let post = desk.send("Bigger title", &keys, from()).unwrap();
    assert_eq!(desk.attempt.as_ref(), Some(&failed), "a retry of the same text keeps its id");
    let result = post.run();
    assert_eq!(result, Err(SendError::Offline));
    desk.settle(result).unwrap();
    assert_eq!(desk.items().count(), 1);
    assert_eq!(load_sent(&paths).len(), 1);
}

/// What stops a send before the network, and that only one send is out at a time.
#[test]
fn a_send_stops_before_the_network_when_it_should() {
    let mut desk = Desk::open(temp("stop"), Build::with_records(None, []));
    let none = MemoryKeys::default();
    let keys = keys_with(&format!("https://hooks.example/inkling#k={SECRET}"));
    assert!(desk.send("Bigger margins", &none, from()).is_none());
    assert_eq!(desk.delivery(), &Delivery::Stopped(CantSend::NoLink));
    assert!(desk.send(" \n ", &keys, from()).is_none());
    assert_eq!(desk.delivery(), &Delivery::Stopped(CantSend::Empty));
    desk.edited();
    assert_eq!(desk.delivery(), &Delivery::Idle, "typing clears \"write something first\"");
    assert!(desk.send(&"a".repeat(MAX_TEXT + 1), &keys, from()).is_none());
    assert_eq!(desk.delivery(), &Delivery::Stopped(CantSend::TooLong));
    desk.edited();
    assert_eq!(desk.delivery(), &Delivery::Stopped(CantSend::TooLong), "only sending again clears the other stops");
    let _post = desk.send("Bigger margins", &keys, from()).unwrap();
    assert!(desk.send("Bigger margins", &keys, from()).is_none());
    assert_eq!(desk.delivery(), &Delivery::Sending);
}

#[test]
fn the_sent_log_survives_a_reload() {
    let paths = temp("log");
    let keys = keys_with(&format!("https://hooks.example/inkling#k={SECRET}"));
    let mut desk = Desk::open(paths.clone(), Build::with_records(None, []));
    desk.fake = true;
    for text in ["First", "Second"] {
        let result = desk.send(text, &keys, from()).unwrap().run();
        desk.settle(result).unwrap();
    }
    let texts = |desk: &Desk| desk.items().map(|i| i.sent.text.clone()).collect::<Vec<_>>();
    assert_eq!(texts(&Desk::open(paths.clone(), Build::with_records(None, []))), ["Second", "First"]);

    let log = r#"[{"id":"1762899330-k7q2m3xd","text":"Kept"},{"id":"nope","text":"x"},7,
                  {"id":"1762899330-k7q2m3xd","text":"Duplicate"},{"id":"1762899331-aaaaaaaa","text":"Also kept"}]"#;
    std::fs::write(paths.sent_feedback(), log).unwrap();
    assert_eq!(texts(&Desk::open(paths.clone(), Build::with_records(None, []))), ["Also kept", "Kept"]);
    std::fs::write(paths.sent_feedback(), "not json").unwrap();
    assert_eq!(Desk::open(paths, Build::with_records(None, [])).items().count(), 0);
}

/// The record format, through the one parser the build script also uses.
#[test]
fn records_decide_status() {
    let (a, b, c) = (id("1762899330-k7q2m3xd"), id("1762899871-p4w2c6ta"), id("1762899999-aaaaaaaa"));
    let done = Record { id: a, outcome: Outcome::Done { summary: "Margins are wider.".into() }, seconds_to_ship: 412 };
    let declined = Record { id: b, outcome: Outcome::Declined { reason: "Already the largest.".into() }, seconds_to_ship: 95 };
    for r in [&done, &declined] {
        assert_eq!(record::parse(&r.id.to_string(), record::to_json(r).as_bytes()), Ok(r.clone()));
    }
    let all = format!("[{},{}]", record::to_json(&done), record::to_json(&declined));
    assert_eq!(record::parse_all(all.as_bytes()), Ok(vec![done.clone(), declined.clone()]));

    let stem = "1762899330-k7q2m3xd";
    let wire = |rest: &str| format!(r#"{{"id":"{stem}",{rest}}}"#);
    for bad in [
        wire(r#""outcome":"done","summary":"x","seconds_to_ship":1"#).replace(stem, "1762899330-aaaaaaaa"),
        wire(r#""outcome":"maybe","summary":"x","seconds_to_ship":1"#),
        wire(r#""outcome":"done","summary":"x","seconds_to_ship":1,"extra":true"#),
        wire(r#""outcome":"declined","summary":"x","reason":"y","seconds_to_ship":1"#),
        wire(r#""outcome":"done","seconds_to_ship":1"#),
        wire(r#""outcome":"done","summary":"two\nlines","seconds_to_ship":1"#),
        wire(r#""outcome":"done","summary":"   ","seconds_to_ship":1"#),
        wire(&format!(r#""outcome":"done","summary":"{}","seconds_to_ship":1"#, "x".repeat(MAX_LINE + 1))),
    ] {
        assert!(record::parse(stem, bad.as_bytes()).is_err(), "{bad}");
    }

    let dir = temp("records").dir;
    std::fs::create_dir_all(&dir).unwrap();
    let now = UNIX_EPOCH + Duration::from_secs(a.sent_at() + 412);
    let path = write_record(&dir, a, Outcome::Done { summary: " Margins are wider. ".into() }, now).unwrap();
    assert_eq!(record::parse(stem, &std::fs::read(&path).unwrap()), Ok(done.clone()));
    assert!(write_record(&dir, a, Outcome::Declined { reason: "a\nb".into() }, now).is_err());

    let build = Build::with_records(None, [done, declined]);
    let status = |id| build.status(id);
    assert_eq!(status(a), Status::Done { summary: "Margins are wider.", seconds_to_ship: 412 });
    assert_eq!(status(b), Status::Declined { reason: "Already the largest." });
    assert_eq!(status(c), Status::Pending);
    assert_eq!(Build::running().number, None, "a test build has no build number");
}
