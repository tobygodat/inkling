//! Headless proof of Account and sync, against the fake server in `fake-server.json`: signed out,
//! Sign in with Google up to the browser and back with a fake code, a document going up and
//! another device's coming down, Sync stopped with its dot and Try again, and a document changed
//! on both devices kept twice.

use super::*;
use inkling_core::Documents;
use inkling_sync::fake::Doc;
use inkling_sync::{FAKE_SERVER, Fake};
use std::io::{Read as _, Write as _};
use std::sync::Mutex;

const MINE: &str = "# On keeping accounts\n\nEvery list I keep grows until it is the work itself.";
const THEIRS: &str = "# Letter to the board\n\nDear all, written on the phone.";

impl Snap {
    /// Lets the app's background work run, in real time, until `until` holds.
    fn wait_for(&mut self, ms: u64, until: impl Fn(&Inkling) -> bool) -> bool {
        for _ in 0..(ms / 10) {
            if self.read(|v, _| until(v)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
            self.advance(16);
        }
        self.read(|v, _| until(v))
    }
}

fn titles(paths: &Paths) -> Vec<String> {
    Documents::new(paths).list().into_iter().map(|d| d.title).collect()
}

pub fn run(dir: &Path) {
    let paths = Paths::mac();
    seed(&paths, MINE);
    let server = Fake::at(paths.dir.join("fake-server.json"));
    // What the phone already sent up.
    server.with(|s| {
        s.documents.insert("1700000000000".into(), Doc { text: THEIRS.into(), saved_at: 1_700_000_000_000, device: "phone-a1b2".into(), version: 1, deleted: false });
    });

    let mut s = Snap::open(dir);
    s.hover("cog");
    s.settle();
    check(s.bounds("menu-account").is_some(), "the cog's menu has an Account row");
    s.click("menu-account");
    check(s.read(|v, _| v.account_open() && !v.signed_in()), "Account opens, signed out");
    check(s.bounds("sign-in").is_some() && s.bounds("account-google").is_none(), "signed out, Account shows only Sign in with Google");
    s.shot("account-01-signed-out");

    // The browser: the address it was asked to open, then the way back with a fake code.
    let opened: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = opened.clone();
    s.update(|v, _, _| {
        v.set_account_host(inkling_ui::AccountHost {
            way_back: inkling_ui::WayBack::Loopback,
            open: Box::new(move |url, _| {
                seen.lock().unwrap().push(url.clone());
                let back = inkling_sync::fake_way_back(&url).expect("the sign-in page names its way back");
                std::thread::spawn(move || {
                    let target = back.split_once("127.0.0.1:").map(|(_, rest)| rest.to_string()).unwrap();
                    let (port, path) = target.split_once('/').unwrap();
                    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port.parse::<u16>().unwrap())).unwrap();
                    write!(stream, "GET /{path} HTTP/1.1\r\nhost: 127.0.0.1\r\n\r\n").unwrap();
                    let mut page = String::new();
                    let _ = stream.read_to_string(&mut page);
                });
                Task::ready(None)
            }),
        })
    });
    s.click("sign-in");
    check(s.wait_for(10_000, |v| v.signed_in()), "the browser comes back with a code and the Mac is signed in");
    let url = opened.lock().unwrap().first().cloned().unwrap_or_default();
    check(url.starts_with(&format!("{FAKE_SERVER}/authorize?")), "Sign in with Google opens the server's sign-in page in the browser");
    check(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A") && url.contains("code_challenge_method=S256") && url.contains("scope=sync%20offline_access"), "it asks for sync with PKCE, coming back to 127.0.0.1");
    check(s.wait_for(10_000, |v| !v.syncing()), "a sync runs at once");
    check(!s.read(|v, _| v.sync_stopped()), "and succeeds");
    check(server.with(|s| s.documents.get("draft").map(|d| d.text.clone())).as_deref() == Some(MINE), "the Mac's document went up");
    check(titles(&paths).contains(&"Letter to the board".to_string()), "the phone's document came down");
    check(["account-readwise", "account-agents", "account-google"].iter().all(|id| s.bounds(*id).is_some()) && s.bounds("account-stopped").is_none(), "signed in, Account shows Readwise, Agents and Google");
    s.click("copy-address");
    let copied = s.cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
    check(copied.is_none_or(|text| text == format!("{FAKE_SERVER}/mcp")), "Copy address copies where agents connect");
    s.click("readwise-connect");
    s.input("rw-token-fake");
    s.click("readwise-connect");
    check(s.wait_for(5_000, |v| v.readwise_connected()), "Readwise connects with nothing but its token");
    s.shot("account-02-signed-in");

    server.with(|s| s.down = true);
    s.update(|v, _, cx| v.sync_now(cx));
    check(s.wait_for(10_000, |v| v.sync_stopped()), "a sync that can't reach the server stops");
    check(s.bounds("sync-dot").is_some() && s.bounds("account-stopped").is_some(), "the cog shows its dot and Account a first row, Sync stopped");
    s.shot("account-03-sync-stopped");
    server.with(|s| s.down = false);
    s.click("sync-again");
    check(s.wait_for(10_000, |v| !v.sync_stopped() && !v.syncing()), "Try again syncs");
    check(s.bounds("sync-dot").is_none() && s.bounds("account-stopped").is_none(), "and the dot and the row go");

    // The same document changed on the phone and here before either synced.
    server.with(|s| {
        let doc = s.documents.get_mut("draft").unwrap();
        doc.text = format!("{MINE}\n\nAdded on the phone.");
        doc.version += 1;
        doc.device = "phone-a1b2".into();
        doc.saved_at = inkling_sync::now_millis() - 60_000;
    });
    s.press("cmd-1");
    s.select(MINE.len()..MINE.len());
    s.input(" Added on the Mac.");
    s.advance(501);
    s.update(|v, _, cx| v.sync_now(cx));
    check(s.wait_for(10_000, |v| !v.syncing()), "the next sync runs");
    let kept = titles(&paths).iter().filter(|t| *t == "On keeping accounts").count();
    check(kept == 2, "a document changed on both devices is kept twice");
    check(s.text().ends_with("Added on the Mac."), "the page keeps what was typed here");
    let texts: Vec<String> = Documents::new(&paths).list().iter().filter_map(|d| Documents::new(&paths).read(&d.id).ok()).collect();
    check(texts.iter().any(|t| t.ends_with("Added on the phone.")), "the phone's copy is the second document");
    check(server.with(|s| s.documents.values().filter(|d| d.text.starts_with("# On keeping")).count()) == 2, "and both are on the server for the phone");
    s.click("open-documents");
    s.shot("account-04-both-kept");

    s.hover("cog");
    s.settle();
    s.click("menu-account");
    s.click("sign-out");
    check(s.read(|v, _| !v.signed_in()) && s.bounds("sign-in").is_some(), "Sign out goes back to Sign in with Google");
}
