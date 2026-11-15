//! The documents: the list, and the page moving between documents. Each document opened since
//! launch keeps its page, so its undo history and its review stay with it while another is open.

use crate::app::{Inkling, Saved, Screen, Version, ghost_button, primary_button};
use crate::library::outlined_button;
use crate::page::Page;
use crate::theme::Theme;
use crate::FormFactor;
use chrono::{DateTime, Datelike, Local};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_core::{Document, Documents, Revision};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

gpui_kit::actions!(inkling, [NewDocument]);

/// The height of the bar at the top of the list, as a book's.
const TOP_BAR: Pixels = px(56.);
/// How far a row on iPhone is swiped left before it is removed.
const SWIPE: Pixels = px(96.);
/// A gap this long between scroll events starts a new swipe.
const SWIPE_PAUSE: Duration = Duration::from_millis(120);

/// A document that is not on the page: its page, with its undo history, and its review.
pub(crate) struct Parked {
    page: Page,
    edited: Subscription,
    revision: Revision,
    version: Version,
}

/// The documents on disk and the ones opened since launch.
pub(crate) struct Docs {
    pub store: Documents,
    /// The id of the document on the page. It may have no file yet: a new page is written once
    /// it has text.
    pub open: String,
    /// Every other document opened since launch.
    parked: HashMap<String, Parked>,
}

impl Docs {
    /// Moves `draft.md` in, then picks the document open last, or a new one.
    pub fn open(paths: &inkling_core::Paths, last: Option<&str>) -> (Self, Result<(), String>) {
        let store = Documents::new(paths);
        let migrated = store.migrate(paths).map_err(|e| format!("Couldn't move draft.md into your documents ({e})."));
        let open = store.last_open(last).unwrap_or_else(|| store.new_id());
        (Docs { store, open, parked: HashMap::new() }, migrated)
    }

    pub fn text(&self, id: &str) -> String {
        self.store.read(id).unwrap_or_default()
    }

    /// Documents sync must leave alone: those parked with a review open.
    pub fn held(&self) -> std::collections::HashSet<String> {
        self.parked.iter().filter(|(_, p)| p.revision.review().is_some() || p.revision.is_busy()).map(|(id, _)| id.clone()).collect()
    }

    /// Drops a parked page whose file sync changed, so it opens again from the file.
    pub fn forget(&mut self, id: &str) {
        self.parked.remove(id);
    }
}

/// The list while it is open.
pub(crate) struct DocList {
    pub entries: Vec<Document>,
    /// The row Delete and Enter act on. It starts on the open document.
    pub chosen: Option<usize>,
    /// The document removed last, until the next action: Cmd-Z or Undo puts it back.
    removed: Option<Removed>,
    /// iPhone: the row being swiped and how far.
    swipe: Option<(usize, Pixels)>,
    /// iPhone: a swipe removes one row. The rest of its stream of scroll events, the fling after
    /// the finger lifts included, is passed over until it pauses or a new touch starts.
    swiped: bool,
    last_scroll: Option<Instant>,
}

struct Removed {
    id: String,
    text: String,
    changed: SystemTime,
    parked: Option<Parked>,
}

impl Inkling {
    /// "‹ Documents" on the page.
    pub fn open_documents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
        self.save_now(cx);
        if self.text(cx).is_empty() {
            let _ = self.docs.store.remove(&self.docs.open);
        }
        let entries = self.docs.store.list();
        let chosen = entries.iter().position(|d| d.id == self.docs.open);
        self.screen = Screen::Documents(DocList { entries, chosen, removed: None, swipe: None, swiped: false, last_scroll: None });
        self.focus.focus(window, cx);
        window.dismiss_virtual_keyboard();
        cx.notify();
    }

    pub fn documents_open(&self) -> bool {
        matches!(self.screen, Screen::Documents(_))
    }

    /// The list as it shows, newest first, and the row chosen.
    pub fn document_list(&self) -> Option<(&[Document], Option<usize>)> {
        match &self.screen {
            Screen::Documents(list) => Some((&list.entries, list.chosen)),
            _ => None,
        }
    }

    /// The id of the document on the page.
    pub fn open_document_id(&self) -> &str {
        &self.docs.open
    }

    /// The page's document as a file in the folder.
    pub fn open_document_path(&self) -> std::path::PathBuf {
        self.docs.store.path(&self.docs.open)
    }

    /// New, and Cmd-N anywhere: an empty page with the caret in it.
    pub fn new_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
        let id = self.docs.store.new_id();
        self.switch_to(id, window, cx);
        self.show_page(window, cx);
        if self.form == FormFactor::Phone {
            self.page.focus_handle(cx).focus(window, cx);
            window.request_virtual_keyboard();
        }
    }

    pub(crate) fn on_new_document(&mut self, _: &NewDocument, window: &mut Window, cx: &mut Context<Self>) {
        self.new_document(window, cx);
    }

    /// Opens a row of the list on the page.
    pub fn open_document(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if id != self.docs.open {
            self.switch_to(id, window, cx);
        }
        self.show_page(window, cx);
    }

    fn show_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.screen = Screen::Page;
        self.focus_page(window, cx);
        cx.notify();
    }

    /// Puts document `id` on the page. The one leaving is saved and kept with its review, unless
    /// it is empty, when its file goes. A request still running is cancelled.
    fn switch_to(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
        self.stop_asking(cx);
        let incoming = match self.docs.parked.remove(&id) {
            Some(parked) => parked,
            None => {
                let text = self.docs.text(&id);
                self.fresh_page(text, window, cx)
            }
        };
        let theme = Theme::new(self.form, &self.settings.appearance, &self.faces);
        incoming.page.set_look(&self.settings.appearance, theme, cx);
        let outgoing = Parked {
            page: std::mem::replace(&mut self.page, incoming.page),
            edited: std::mem::replace(&mut self.page_edited, incoming.edited),
            revision: std::mem::replace(&mut self.revision, incoming.revision),
            version: std::mem::replace(&mut self.version, incoming.version),
        };
        let left = std::mem::replace(&mut self.docs.open, id);
        if outgoing.page.value(cx).is_empty() {
            let _ = self.docs.store.remove(&left);
        } else {
            self.docs.parked.insert(left, outgoing);
        }
        self.saved = Saved::Saved;
        self.settings.document = Some(self.docs.open.clone());
        if let Err(e) = self.settings.save(&self.paths) {
            self.notice = Some(format!("Couldn't save which document is open ({e}).").into());
        }
        cx.notify();
    }

    /// Sync changed the open document's file: the page shows it afresh, without the old undo
    /// history, which would undo text the writer never typed here.
    pub(crate) fn reload_page(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let fresh = self.fresh_page(text, window, cx);
        let focused = self.page.focus_handle(cx).is_focused(window);
        self.page = fresh.page;
        self.page_edited = fresh.edited;
        self.saved = Saved::Saved;
        if focused {
            self.page.focus_handle(cx).focus(window, cx);
        }
    }

    fn fresh_page(&self, text: String, window: &mut Window, cx: &mut Context<Self>) -> Parked {
        let theme = Theme::new(self.form, &self.settings.appearance, &self.faces);
        let page = Page::new(self.form, text, &self.settings.appearance, theme, self.docs.store.dir().to_path_buf(), window, cx);
        let edited = page.on_edit(window, cx, Self::edited);
        Parked { page, edited, revision: Revision::Idle, version: Version { number: 1, source: self.settings.provider } }
    }

    /// Writes the page now if an autosave is waiting.
    pub(crate) fn save_now(&mut self, cx: &mut Context<Self>) {
        if self.saved != Saved::Pending {
            return;
        }
        self.save = Task::ready(());
        self.saved = match self.docs.store.write(&self.docs.open, &self.text(cx)) {
            Ok(()) => Saved::Saved,
            Err(e) => Saved::Failed(e.to_string()),
        };
    }

    /// Moves the chosen row up or down.
    fn choose_row(&mut self, down: bool, cx: &mut Context<Self>) {
        let Screen::Documents(list) = &mut self.screen else { return };
        let last = list.entries.len().checked_sub(1);
        list.chosen = match (list.chosen, last) {
            (_, None) => None,
            (None, Some(_)) => Some(0),
            (Some(at), Some(last)) if down => Some((at + 1).min(last)),
            (Some(at), Some(_)) => Some(at.saturating_sub(1)),
        };
        cx.notify();
    }

    pub fn choose_document(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Screen::Documents(list) = &mut self.screen {
            list.chosen = (at < list.entries.len()).then_some(at);
            cx.notify();
        }
    }

    fn open_chosen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Screen::Documents(list) = &self.screen else { return };
        if let Some(entry) = list.chosen.and_then(|at| list.entries.get(at)) {
            self.open_document(entry.id.clone(), window, cx);
        }
    }

    /// Delete on the Mac, a swipe on iPhone. The document can come back until the next action.
    pub fn remove_document(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Screen::Documents(list) = &self.screen else { return };
        let Some(entry) = list.entries.get(at).cloned() else { return };
        let text = self.docs.text(&entry.id);
        if let Err(e) = self.docs.store.remove(&entry.id) {
            self.notice = Some(format!("Couldn't delete {} ({e}).", shown_title(&entry.title)).into());
            return cx.notify();
        }
        if entry.id == self.docs.open {
            let next = list.entries.iter().find(|d| d.id != entry.id).map(|d| d.id.clone());
            let next = next.unwrap_or_else(|| self.docs.store.new_id());
            self.switch_to(next, window, cx);
        }
        let parked = self.docs.parked.remove(&entry.id);
        self.refresh_list(None);
        if let Screen::Documents(list) = &mut self.screen {
            list.removed = Some(Removed { id: entry.id, text, changed: entry.changed, parked });
            list.chosen = (!list.entries.is_empty()).then(|| at.min(list.entries.len() - 1));
        }
        self.sync_soon(cx);
        cx.notify();
    }

    fn remove_chosen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Screen::Documents(DocList { chosen: Some(at), .. }) = self.screen {
            self.remove_document(at, window, cx);
        }
    }

    /// Cmd-Z on the list, or Undo on iPhone: the document removed last comes back where it was.
    pub fn undo_remove(&mut self, cx: &mut Context<Self>) {
        let Screen::Documents(list) = &mut self.screen else { return };
        let Some(removed) = list.removed.take() else { return };
        if let Err(e) = self.docs.store.restore(&removed.id, &removed.text, removed.changed) {
            self.notice = Some(format!("Couldn't bring the document back ({e}).").into());
            return cx.notify();
        }
        if let Some(parked) = removed.parked {
            self.docs.parked.insert(removed.id.clone(), parked);
        }
        self.sync_soon(cx);
        self.refresh_list(Some(&removed.id));
        cx.notify();
    }

    pub fn can_undo_remove(&self) -> bool {
        matches!(&self.screen, Screen::Documents(DocList { removed: Some(_), .. }))
    }

    fn refresh_list(&mut self, choose: Option<&str>) {
        let entries = self.docs.store.list();
        if let Screen::Documents(list) = &mut self.screen {
            if let Some(id) = choose {
                list.chosen = entries.iter().position(|d| d.id == id);
            }
            list.entries = entries;
            list.swipe = None;
        }
    }

    /// The keys the list owns: the arrows choose, Enter opens, Delete removes, Cmd-Z undoes it and
    /// Escape goes back to the page.
    pub(crate) fn documents_key(&mut self, key: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !matches!(self.screen, Screen::Documents(_)) || self.typing_hex(window, cx) {
            return false;
        }
        let m = key.modifiers;
        match key.key.as_str() {
            "z" if m.platform && !m.shift => self.undo_remove(cx),
            _ if m.modified() => return false,
            "up" => self.choose_row(false, cx),
            "down" => self.choose_row(true, cx),
            "enter" => self.open_chosen(window, cx),
            "backspace" | "delete" => self.remove_chosen(window, cx),
            _ => return false,
        }
        true
    }

    /// iPhone: a row follows the finger left, and is removed once it has gone far enough.
    fn swiped_row(&mut self, at: usize, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let Screen::Documents(list) = &mut self.screen else { return };
        let paused = list.last_scroll.is_none_or(|at| now.duration_since(at) >= SWIPE_PAUSE);
        list.last_scroll = Some(now);
        if paused || event.touch_phase == TouchPhase::Started {
            list.swiped = false;
        }
        if list.swiped {
            return;
        }
        if matches!(event.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            list.swipe = None;
            return cx.notify();
        }
        let delta = event.delta.pixel_delta(px(20.));
        if delta.x.abs() <= delta.y.abs() {
            return;
        }
        let gone = match list.swipe {
            Some((row, x)) if row == at => (x + delta.x).min(px(0.)),
            _ => delta.x.min(px(0.)),
        };
        if gone <= -SWIPE {
            list.swiped = true;
            self.remove_document(at, window, cx);
        } else {
            list.swipe = Some((at, gone));
            cx.notify();
        }
    }

    /// "‹ Documents", after the corner.
    pub(crate) fn render_documents_link(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = self.theme().c;
        ghost_button(&c, "open-documents", "‹ Documents")
            .flex_none()
            .h(self.metrics().control.max(px(32.)))
            // The phone's header is full: the link sits close to the corner.
            .when(self.form == FormFactor::Phone, |link| link.ml(px(-8.)).px(px(4.)).text_color(c.ink))
            .on_click(cx.listener(|this, _, window, cx| this.open_documents(window, cx)))
            .test_support()
            .into_any_element()
    }

    pub(crate) fn render_documents(&self, list: &DocList, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = self.theme();
        let c = t.c;
        let m = self.metrics();
        let phone = self.form == FormFactor::Phone;
        let top = div()
            .w_full()
            .h(if phone { m.control } else { TOP_BAR })
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .pl(self.corner_width())
            .pr(if phone { px(8.) } else { m.gutter })
            .child(
                primary_button(&c, "new-document", "New", m)
                    .on_click(cx.listener(|this, _, window, cx| this.new_document(window, cx)))
                    .test_support(),
            );
        let now = Local::now();
        let rows = list.entries.iter().enumerate().map(|(at, d)| {
            let chosen = list.chosen == Some(at);
            let open = d.id == self.docs.open;
            let shift = list.swipe.filter(|(row, _)| *row == at).map_or(px(0.), |(_, x)| x);
            let id = d.id.clone();
            div()
                .id(("document", at))
                .w_full()
                .min_h(m.control.max(px(40.)))
                .px(px(10.))
                .ml(shift)
                .flex()
                .items_center()
                .gap_3()
                .rounded(px(8.))
                .when(chosen || (phone && open), |row| row.bg(c.segment))
                .cursor_pointer()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(t.body.face)
                        .text_size(px(if phone { 17. } else { 18. }))
                        .font_weight(FontWeight::SEMIBOLD)
                        .when(d.title.is_empty(), |title| title.text_color(c.muted))
                        .child(shown_title(&d.title).to_string()),
                )
                .child(div().flex_none().text_size(px(12.)).text_color(c.muted).child(day(d.changed, now)))
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    if this.form == FormFactor::Phone || event.click_count() > 1 {
                        this.open_document(id.clone(), window, cx);
                    } else {
                        this.choose_document(at, cx);
                    }
                }))
                .when(phone, |row| row.on_scroll(cx.listener(move |this, event: &ScrollEvent, window, cx| this.swiped_row(at, event, window, cx))))
                .test_support()
        });
        let column = div().w_full().max_w(t.body.column).flex().flex_col().gap(px(2.)).children(rows);
        let undo = (phone && list.removed.is_some()).then(|| {
            div().w_full().px(m.gutter).pb_2().child(
                outlined_button(&c, "undo-remove", "Undo")
                    .w_full()
                    .h(m.control)
                    .justify_center()
                    .text_size(px(15.))
                    .on_click(cx.listener(|this, _, _, cx| this.undo_remove(cx)))
                    .test_support(),
            )
        });
        div()
            .id("documents")
            .size_full()
            .flex()
            .flex_col()
            .child(top)
            .child(
                div()
                    .id("document-list")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .flex()
                    .justify_center()
                    .px(if phone { px(8.) } else { m.gutter })
                    .pt(if phone { px(8.) } else { px(24.) })
                    .child(column)
                    .test_support(),
            )
            .children(undo)
            .test_support()
    }
}

/// An empty first line still needs a name in the list.
fn shown_title(title: &str) -> &str {
    if title.is_empty() { "Untitled" } else { title }
}

/// "Today", "Yesterday", "3 Nov", or "12 Oct 2025" before this year.
fn day(changed: SystemTime, now: DateTime<Local>) -> String {
    let date = DateTime::<Local>::from(changed).date_naive();
    let today = now.date_naive();
    if date == today {
        "Today".into()
    } else if today.pred_opt() == Some(date) {
        "Yesterday".into()
    } else if date.year() == today.year() {
        date.format("%-d %b").to_string()
    } else {
        date.format("%-d %b %Y").to_string()
    }
}
