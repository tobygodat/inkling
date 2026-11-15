//! Account: signing in to Inkling's server with Google, and sync, which then runs on its own at
//! launch, when the app comes to the front, a moment after a change settles and every few minutes
//! while it is open (ADR 0004). Nothing shows while it works. When a sync fails, the cog carries a
//! small ink dot and Account a "Sync stopped" row with Try again, until a sync succeeds.
//!
//! Sync itself is `inkling_sync`. It runs on the background executor and returns what to change;
//! this file makes those changes on the UI thread, where the page and the library live.

use crate::app::{Inkling, Menu, Saved, Screen, primary_button};
use crate::library::outlined_button;
use crate::FormFactor;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_core::{Documents, Secret};
use inkling_sync::{Disk, Error, SignIn, Synced, Transport};
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, UNIX_EPOCH};

/// How long after the last change a sync starts.
const SETTLE: Duration = Duration::from_secs(3);
/// How often sync runs while the app is open, for what other devices and Readwise sent.
const EVERY: Duration = Duration::from_secs(5 * 60);
/// How long the Mac waits for the browser to come back from signing in.
const SIGN_IN_LIMIT: Duration = Duration::from_secs(10 * 60);

/// Shows the server's sign-in page. With [`WayBack::Loopback`] the page comes back to a listener
/// the app holds, and the task's answer is not used; with [`WayBack::Scheme`] the task resolves
/// to the address the sheet came back to, or `None` when the writer cancelled.
pub type OpenSignIn = Box<dyn Fn(String, &mut App) -> Task<Option<String>>>;

/// Where the sign-in page sends the browser back to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WayBack {
    /// A listener on 127.0.0.1: the Mac, with the system browser.
    Loopback,
    /// An address the host's sign-in sheet catches: the iPhone, with `inkling://callback`.
    Scheme(&'static str),
}

/// What the host lends Account, as it lends the library a `BookHost`.
pub struct AccountHost {
    pub way_back: WayBack,
    pub open: OpenSignIn,
}

impl AccountHost {
    /// The system browser through GPUI, coming back to a listener: the Mac's way.
    pub fn browser() -> Self {
        AccountHost {
            way_back: WayBack::Loopback,
            open: Box::new(|url, cx| {
                cx.open_url(&url);
                Task::ready(None)
            }),
        }
    }
}

pub(crate) struct Account {
    transport: Arc<dyn Transport>,
    server: &'static str,
    pub host: AccountHost,
    /// Whether a sign-in is kept, asked of the key store at launch and on each change.
    pub signed_in: bool,
    /// The last sync failed. Cleared by the next that succeeds.
    pub stopped: bool,
    /// Whether Readwise is connected on the server, as the last sync found it.
    pub readwise: bool,
    /// The Readwise token field, while Connect is open.
    pub readwise_field: Option<(Entity<InputState>, Subscription)>,
    /// The address an agent connects to, once copied, so the button can say so.
    pub copied: bool,
    /// Why the last sign-in or Readwise change didn't work.
    pub problem: Option<SharedString>,
    state: inkling_sync::State,
    syncing: Option<Task<()>>,
    /// A sync was asked for while one ran: run again when it ends.
    again: bool,
    soon: Task<()>,
    signing_in: Option<(Arc<AtomicBool>, Task<()>)>,
    busy: Task<()>,
}

impl Account {
    pub fn new(paths: &inkling_core::Paths, form: FormFactor, keys: &dyn inkling_core::KeyStore) -> Self {
        let (transport, server) = inkling_sync::transport_for(paths);
        Account {
            transport: Arc::from(transport),
            server,
            host: AccountHost::browser(),
            signed_in: keys.has(Secret::ServerSignIn),
            stopped: false,
            readwise: false,
            readwise_field: None,
            copied: false,
            problem: None,
            state: inkling_sync::load_state(paths, form.id()),
            syncing: None,
            again: false,
            soon: Task::ready(()),
            signing_in: None,
            busy: Task::ready(()),
        }
    }
}

impl Inkling {
    /// The host's way to sign in. Without one, the system browser through GPUI.
    pub fn set_account_host(&mut self, host: AccountHost) {
        self.account.host = host;
    }

    pub fn open_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
        self.screen = Screen::Account;
        self.panel = None;
        self.menu = Menu::Closed;
        self.account.copied = false;
        self.account.problem = None;
        self.focus.focus(window, cx);
        window.dismiss_virtual_keyboard();
        cx.notify();
    }

    pub fn account_open(&self) -> bool {
        matches!(self.screen, Screen::Account)
    }

    pub fn signed_in(&self) -> bool {
        self.account.signed_in
    }

    /// Whether the last sync failed, so the cog shows its dot.
    pub fn sync_stopped(&self) -> bool {
        self.account.stopped
    }

    pub fn syncing(&self) -> bool {
        self.account.syncing.is_some()
    }

    pub fn readwise_connected(&self) -> bool {
        self.account.readwise
    }

    pub fn account_problem(&self) -> Option<&str> {
        self.account.problem.as_deref()
    }

    /// Starts the jobs that run sync without being asked: one now, one each time the window
    /// comes to the front, and one every few minutes.
    pub(crate) fn start_syncing(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Subscription {
        cx.spawn(async move |this, cx| {
            loop {
                if this.update(cx, |this, cx| this.sync_now(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(EVERY).await;
            }
        })
        .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.sync_now(cx);
            }
        })
    }

    /// A change settled: sync a moment after the last one.
    pub(crate) fn sync_soon(&mut self, cx: &mut Context<Self>) {
        if !self.account.signed_in {
            return;
        }
        self.account.soon = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let _ = this.update(cx, |this, cx| this.sync_now(cx));
        });
    }

    /// Syncs now, or as soon as the sync running ends. Try again too.
    pub fn sync_now(&mut self, cx: &mut Context<Self>) {
        if !self.account.signed_in {
            return;
        }
        if self.account.syncing.is_some() {
            self.account.again = true;
            return;
        }
        let Some(mut sign_in) = SignIn::load(self.keys.as_ref()) else {
            self.account.signed_in = false;
            return cx.notify();
        };
        self.save_now(cx);
        let transport = self.account.transport.clone();
        let state = self.account.state.clone();
        let paths = self.paths.clone();
        let run = cx.background_spawn(async move {
            let result = inkling_sync::run(transport.as_ref(), &mut sign_in, &state, &paths);
            (sign_in, result)
        });
        let window = self.window;
        self.account.syncing = Some(cx.spawn(async move |this, cx| {
            let (sign_in, result) = run.await;
            let _ = cx.update_window(window, |_, window, cx| {
                let _ = this.update(cx, |this, cx| this.synced(sign_in, result, window, cx));
            });
        }));
        cx.notify();
    }

    fn synced(&mut self, sign_in: SignIn, result: Result<Synced, Error>, window: &mut Window, cx: &mut Context<Self>) {
        self.account.syncing = None;
        if self.account.signed_in && SignIn::load(self.keys.as_ref()).is_some_and(|kept| kept != sign_in && kept.client_id == sign_in.client_id) {
            let _ = sign_in.save(self.keys.as_ref());
        }
        match result {
            Ok(synced) => {
                self.account.stopped = false;
                self.account.readwise = synced.readwise;
                self.take_documents(synced.documents, window, cx);
                self.take_books(synced.library, cx);
            }
            Err(Error::SignedOut) => {
                // The grant ended: revoked, or unused for 60 days. Signing in again starts afresh.
                let _ = self.keys.remove(Secret::ServerSignIn);
                self.account.signed_in = false;
                self.account.stopped = true;
            }
            Err(_) => self.account.stopped = true,
        }
        if std::mem::take(&mut self.account.again) {
            self.sync_now(cx);
        }
        cx.notify();
    }

    /// Writes what other devices changed, each only while its file still holds what sync read.
    /// The page on screen is saved first, so it and its file agree; a document under review or
    /// with a request running is left alone, and its two copies are kept at the next sync.
    fn take_documents(&mut self, outcome: inkling_sync::documents::Outcome, window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
        let mut held: HashSet<String> = self.docs.held();
        if self.revision.review().is_some() || self.revision.is_busy() || self.saved != Saved::Saved {
            held.insert(self.docs.open.clone());
        }
        let mut disk = PageDisk { store: &self.docs.store, held, open: self.docs.open.clone() };
        let touched = inkling_sync::documents::apply(outcome, &mut self.account.state, &mut disk);
        if let Err(e) = inkling_sync::save_state(&self.paths, &self.account.state) {
            self.notice = Some(format!("Couldn't save what sync agreed ({e}).").into());
        }
        for id in &touched {
            self.docs.forget(id);
        }
        if touched.contains(&self.docs.open) {
            let text = self.docs.text(&self.docs.open);
            self.reload_page(text, window, cx);
        }
        if let Screen::Documents(list) = &mut self.screen {
            list.entries = self.docs.store.list();
        }
    }

    /// Records the books, places and Readwise uploads sync brought.
    fn take_books(&mut self, outcome: inkling_sync::library::Outcome, cx: &mut Context<Self>) {
        let Ok(library) = self.books.library.as_mut() else { return };
        for entry in outcome.books {
            let _ = library.record(entry);
        }
        for (id, position, at) in outcome.positions {
            let _ = library.take_position(&id, position, at);
        }
        let added = !outcome.imported.is_empty();
        for prepared in outcome.imported {
            let _ = library.add(&prepared);
        }
        if added {
            // The new books go up at once.
            self.sync_soon(cx);
        }
    }

    pub fn sign_in(&mut self, cx: &mut Context<Self>) {
        if let Some((cancelled, _)) = self.account.signing_in.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.account.problem = None;
        let cancelled = Arc::new(AtomicBool::new(false));
        let transport = self.account.transport.clone();
        let server = self.account.server;
        let way_back = self.account.host.way_back;
        let client = match self.form {
            FormFactor::Mac => "Inkling for Mac",
            FormFactor::Phone => "Inkling for iPhone",
        };
        let stop = cancelled.clone();
        let task = cx.spawn(async move |this, cx| {
            let begin = {
                let transport = transport.clone();
                cx.background_spawn(async move {
                    let (loopback, redirect) = match way_back {
                        WayBack::Loopback => {
                            let listener = inkling_sync::Loopback::bind()?;
                            let redirect = listener.redirect();
                            (Some(listener), redirect)
                        }
                        WayBack::Scheme(address) => (None, address.to_string()),
                    };
                    inkling_sync::account::begin(transport.as_ref(), server, &redirect, client).map(|begun| (begun, loopback))
                })
            };
            let signed = async {
                let (begun, loopback) = begin.await?;
                let shown = this.update(cx, |this, cx| (this.account.host.open)(begun.url.clone(), cx)).map_err(|_| Error::Local(String::new()))?;
                let came_back = match loopback {
                    Some(loopback) => {
                        let stop = stop.clone();
                        cx.background_spawn(async move { loopback.wait(&stop, SIGN_IN_LIMIT) }).await?
                    }
                    None => shown.await.ok_or_else(|| Error::Local(String::new()))?,
                };
                let transport = transport.clone();
                cx.background_spawn(async move { begun.finish(transport.as_ref(), &came_back) }).await
            }
            .await;
            let _ = this.update(cx, |this, cx| this.signed(signed, cx));
        });
        self.account.signing_in = Some((cancelled, task));
        cx.notify();
    }

    fn signed(&mut self, signed: Result<SignIn, Error>, cx: &mut Context<Self>) {
        self.account.signing_in = None;
        match signed.and_then(|s| s.save(self.keys.as_ref()).map_err(Error::Local)) {
            Ok(()) => {
                self.account.signed_in = true;
                self.account.stopped = false;
                self.sync_now(cx);
            }
            // Cancelled, or the browser never came back: nothing to say.
            Err(Error::Local(why)) if why.is_empty() || why.starts_with("The sign-in wasn't finished") => {}
            Err(e) => self.account.problem = Some(e.to_string().into()),
        }
        cx.notify();
    }

    pub fn sign_out(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.keys.remove(Secret::ServerSignIn) {
            self.account.problem = Some(e.into());
            return cx.notify();
        }
        self.account.signed_in = false;
        self.account.stopped = false;
        self.account.readwise = false;
        self.account.readwise_field = None;
        self.account.syncing = None;
        cx.notify();
    }

    /// Copy address: where an agent such as Claude or ChatGPT connects.
    pub fn copy_agents_address(&mut self, cx: &mut Context<Self>) {
        let Some(sign_in) = SignIn::load(self.keys.as_ref()) else { return };
        cx.write_to_clipboard(ClipboardItem::new_string(sign_in.agents_address()));
        self.account.copied = true;
        cx.notify();
    }

    /// Connect opens a field for the Readwise token; Connect again sends it. Disconnect when
    /// connected.
    pub fn readwise_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.account.readwise {
            return self.change_readwise(None, cx);
        }
        match &self.account.readwise_field {
            Some((field, _)) => {
                let token = field.read(cx).value().trim().to_string();
                if !token.is_empty() {
                    self.change_readwise(Some(token), cx);
                }
            }
            None => {
                let field = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("Readwise access token"));
                let enter = cx.subscribe_in(&field, window, |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.readwise_button(window, cx);
                        window.dismiss_virtual_keyboard();
                    }
                });
                field.read(cx).focus_handle(cx).focus(window, cx);
                self.account.readwise_field = Some((field, enter));
                cx.notify();
            }
        }
    }

    fn change_readwise(&mut self, token: Option<String>, cx: &mut Context<Self>) {
        let Some(mut sign_in) = SignIn::load(self.keys.as_ref()) else { return };
        let transport = self.account.transport.clone();
        self.account.problem = None;
        let run = cx.background_spawn(async move { inkling_sync::readwise(transport.as_ref(), &mut sign_in, token.as_deref()) });
        self.account.busy = cx.spawn(async move |this, cx| {
            let result = run.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(connected) => {
                        this.account.readwise = connected;
                        this.account.readwise_field = None;
                    }
                    Err(e) => this.account.problem = Some(e.to_string().into()),
                }
                cx.notify();
            });
        });
    }

    pub(crate) fn render_account(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = self.theme().c;
        let m = self.metrics();
        let phone = self.form == FormFactor::Phone;
        let height = if phone { m.control } else { px(32.) };
        let problem = self.account.problem.clone().map(|p| div().id("account-problem").text_size(px(13.)).text_color(c.muted).child(p).test_support());
        let screen = div().id("account").size_full().flex().flex_col().items_center().px(m.gutter).text_size(if phone { px(15.) } else { px(14.) });
        if !self.account.signed_in {
            let busy = self.account.signing_in.is_some();
            return screen
                .justify_center()
                .gap_3()
                .child(
                    primary_button(&c, "sign-in", "Sign in with Google", m)
                        .when(busy, |b| b.opacity(0.6))
                        .on_click(cx.listener(|this, _, _, cx| this.sign_in(cx)))
                        .test_support(),
                )
                .children(problem)
                .test_support();
        }
        let row = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .w_full()
                .min_h(if phone { px(56.) } else { px(52.) })
                .flex()
                .items_center()
                .gap_3()
                .border_b_1()
                .border_color(c.hairline)
                .child(div().flex_1().min_w_0().child(label))
        };
        let button = |id: &'static str, label: &'static str| outlined_button(&c, id, label).h(height).flex_none();
        let readwise_label = if self.account.readwise { "Disconnect" } else { "Connect" };
        let readwise = row("account-readwise", "Readwise")
            .when_some(self.account.readwise_field.as_ref(), |row, (field, _)| {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(height)
                        .px_2()
                        .flex()
                        .items_center()
                        .bg(c.surface)
                        .border_1()
                        .border_color(c.hairline)
                        .rounded(px(8.))
                        .child(Input::new(field).id("readwise-token").appearance(false)),
                )
            })
            .child(button("readwise-connect", readwise_label).on_click(cx.listener(|this, _, window, cx| this.readwise_button(window, cx))).test_support())
            .test_support();
        let copy = if self.account.copied { "Copied" } else { "Copy address" };
        screen
            .pt(if phone { m.control + px(12.) } else { px(72.) })
            .child(
                div()
                    .w_full()
                    .max_w(px(480.))
                    .flex()
                    .flex_col()
                    .when(self.account.stopped, |list| {
                        list.child(
                            row("account-stopped", "Sync stopped").child(
                                primary_button(&c, "sync-again", "Try again", m)
                                    .h(height)
                                    .flex_none()
                                    .on_click(cx.listener(|this, _, _, cx| this.sync_now(cx)))
                                    .test_support(),
                            )
                            .test_support(),
                        )
                    })
                    .child(readwise)
                    .child(row("account-agents", "Agents").child(button("copy-address", copy).on_click(cx.listener(|this, _, _, cx| this.copy_agents_address(cx))).test_support()).test_support())
                    .child(row("account-google", "Google").child(button("sign-out", "Sign out").on_click(cx.listener(|this, _, _, cx| this.sign_out(cx))).test_support()).test_support())
                    .children(problem.map(|p| p.pt_3())),
            )
            .test_support()
    }

    /// The small ink dot on the cog while sync has stopped. Ink, because red is for changes.
    pub(crate) fn render_stopped_dot(&self) -> Option<impl IntoElement + use<>> {
        let c = self.theme().c;
        self.account.stopped.then(|| div().id("sync-dot").absolute().top(px(3.)).right(px(3.)).size(px(6.)).rounded_full().bg(c.ink).test_support())
    }
}

/// The documents folder as sync's [`Disk`]: the open document only while its page agrees with
/// its file, and never one `held`.
struct PageDisk<'a> {
    store: &'a Documents,
    held: HashSet<String>,
    open: String,
}

impl Disk for PageDisk<'_> {
    fn read(&self, id: &str) -> Option<String> {
        self.store.read(id).ok()
    }

    fn write(&mut self, id: &str, text: &str, saved_at: u64) -> Result<(), String> {
        self.store.restore(id, text, UNIX_EPOCH + Duration::from_millis(saved_at)).map_err(|e| e.to_string())
    }

    /// The open document stays while it is on the page: it goes once the writer leaves it.
    fn remove(&mut self, id: &str) -> Result<(), String> {
        if id == self.open {
            return Err("open".into());
        }
        self.store.remove(id).map_err(|e| e.to_string())
    }

    fn open_to_sync(&self, id: &str) -> bool {
        !self.held.contains(id)
    }
}
