//! The Appearance panel: beside the page on the Mac, so every choice shows on the page at once.
//! Each change goes through `Inkling::set_appearance`, which saves it and recolours the kit.

use crate::app::{Inkling, Screen, ghost_button};
use crate::icons::Icon;
use crate::picker::Picker;
use crate::theme::{Colors, Theme};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_core::appearance::{LineSpacing, PageWidth, TextSize};
use inkling_core::{Appearance, Face, Mode, PageMode, Settings, View};

pub(crate) const PANEL_WIDTH: Pixels = px(300.);

/// One of the panel's sliders, and the part of `Appearance` it sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Measure {
    TextSize,
    LineSpacing,
    PageWidth,
}

impl Measure {
    const ALL: [Measure; 3] = [Measure::TextSize, Measure::LineSpacing, Measure::PageWidth];

    fn label(self) -> &'static str {
        match self {
            Measure::TextSize => "Text size",
            Measure::LineSpacing => "Line spacing",
            Measure::PageWidth => "Page width",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Measure::TextSize => "text-size",
            Measure::LineSpacing => "line-spacing",
            Measure::PageWidth => "page-width",
        }
    }

    /// Minimum, maximum and step, in the units `Appearance` stores.
    fn range(self) -> (u16, u16, u16) {
        match self {
            Measure::TextSize => (TextSize::MIN, TextSize::MAX, 1),
            Measure::LineSpacing => (LineSpacing::MIN, LineSpacing::MAX, 5),
            Measure::PageWidth => (PageWidth::MIN, PageWidth::MAX, 20),
        }
    }

    fn get(self, a: &Appearance) -> u16 {
        match self {
            Measure::TextSize => a.text_size.get(),
            Measure::LineSpacing => a.line_spacing.get(),
            Measure::PageWidth => a.page_width.get(),
        }
    }

    fn set(self, a: &mut Appearance, value: f32) {
        let value = f64::from(value);
        match self {
            Measure::TextSize => a.text_size = TextSize::new(value),
            Measure::LineSpacing => a.line_spacing = LineSpacing::new(value),
            Measure::PageWidth => a.page_width = PageWidth::new(value),
        }
    }

    fn show(self, a: &Appearance) -> String {
        match self {
            Measure::TextSize => format!("{} pt", a.text_size.get()),
            Measure::LineSpacing => format!("{:.2}", a.line_height()),
            Measure::PageWidth => format!("{} pt", a.page_width.get()),
        }
    }
}

/// Open while `Inkling::panel` is `Some`, beside the page, the library or a book.
pub(crate) struct AppearancePanel {
    sliders: [Entity<SliderState>; 3],
    /// The colour row that is open, if any.
    pub picker: Option<Picker>,
    /// The view whose look the sliders show.
    view: View,
    _subs: Vec<Subscription>,
}

impl AppearancePanel {
    fn new(appearance: &Appearance, view: View, window: &mut Window, cx: &mut Context<Inkling>) -> Self {
        let mut subs = vec![];
        let sliders = Measure::ALL.map(|measure| {
            let (min, max, step) = measure.range();
            let slider = cx.new(|_| {
                SliderState::new()
                    .min(min.into())
                    .max(max.into())
                    .step(step.into())
                    .default_value(f32::from(measure.get(appearance)))
            });
            subs.push(cx.subscribe_in(&slider, window, move |this, _, event: &SliderEvent, window, cx| {
                if let SliderEvent::Change(value) = event {
                    let value = value.start();
                    this.set_appearance(|a| measure.set(a, value), window, cx);
                }
            }));
            slider
        });
        AppearancePanel { sliders, picker: None, view, _subs: subs }
    }

    /// Moves each slider to what `appearance` holds, after a change that didn't come from it.
    pub(crate) fn follow(&self, appearance: &Appearance, window: &mut Window, cx: &mut App) {
        for (slider, measure) in self.sliders.iter().zip(Measure::ALL) {
            slider.update(cx, |s, cx| s.set_value(f32::from(measure.get(appearance)), window, cx));
        }
    }
}

impl Inkling {
    /// The Appearance row of the cog's menu. The panel opens beside the view on screen and edits
    /// that view's look: the page's, or the library's and the books'.
    pub fn open_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
        if self.panel.is_some() {
            return;
        }
        if self.screen.keys().is_some() || self.screen.feedback().is_some() {
            self.screen = Screen::Page;
            self.focus_page(window, cx);
        }
        self.panel = Some(AppearancePanel::new(self.settings.look(self.view()), self.view(), window, cx));
        cx.notify();
    }

    pub fn close_appearance(&mut self, cx: &mut Context<Self>) {
        self.panel = None;
        cx.notify();
    }

    /// The look of the view on screen, which the panel edits.
    pub fn appearance(&self) -> &Appearance {
        self.settings.look(self.view())
    }

    /// The body faces this device has, in the panel's order.
    pub fn faces(&self) -> &[Face] {
        &self.faces
    }

    pub fn appearance_open(&self) -> bool {
        self.panel.is_some()
    }

    /// Changes the look of the view on screen (and the other's, while they are synced), draws it
    /// everywhere at once and saves it.
    pub fn set_appearance(&mut self, change: impl FnOnce(&mut Appearance), window: &mut Window, cx: &mut Context<Self>) {
        let view = self.view();
        self.change_settings(|s| s.change_look(view, change), window, cx);
    }

    /// Turns "Same look for reading and writing" on or off. Turning it on gives the other view the
    /// look on screen.
    pub fn set_synced(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        let view = self.view();
        self.change_settings(|s| s.set_synced(on, view), window, cx);
    }

    /// Scroll or Paged, for reading.
    pub fn set_page_mode(&mut self, pages: PageMode, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.settings.reading.pages;
        if before != pages {
            self.pages_changed(before);
        }
        self.change_settings(|s| s.reading.pages = pages, window, cx);
    }

    fn change_settings(&mut self, change: impl FnOnce(&mut Settings), window: &mut Window, cx: &mut Context<Self>) {
        let before = self.settings.clone();
        change(&mut self.settings);
        if self.settings == before {
            return;
        }
        if let Err(e) = self.settings.save(&self.paths) {
            self.notice = Some(format!("Couldn't save the appearance ({e}).").into());
        }
        self.theme().apply_to_kit(cx);
        if self.settings.appearance != before.appearance {
            let writing = Theme::new(self.form, &self.settings.appearance, &self.faces);
            self.page.set_look(&self.settings.appearance, writing, cx);
        }
        window.refresh();
        cx.notify();
    }

    /// Reset: the default Light look for the view on screen, with every slider back where it started.
    pub fn reset_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_picker(cx);
        let view = self.view();
        self.set_appearance(
            |a| match view {
                View::Writing => *a = Appearance::default(),
                View::Reading => a.take_look(&Appearance::default()),
            },
            window,
            cx,
        );
        self.follow_view(window, cx);
    }

    /// After the view changes under an open panel, or a reset, its sliders show the look on screen.
    pub(crate) fn follow_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = self.view();
        if let Some(panel) = &mut self.panel {
            panel.follow(self.settings.look(view), window, cx);
            panel.view = view;
        }
    }

    /// Called on every frame: a panel left showing the other view's look catches up.
    pub(crate) fn panel_follows_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel.as_ref().is_some_and(|p| p.view != self.view()) {
            self.follow_view(window, cx);
        }
    }

    /// "Same look for reading and writing": while on, a change in either view shows in both.
    fn render_sync(&self, c: &Colors, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let on = self.settings.reading.synced;
        let view = cx.entity().downgrade();
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .p(px(10.))
            .rounded(px(10.))
            .bg(c.segment)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(13.))
                    .child(Icon::Link.draw(px(15.), if on { c.link } else { c.muted }))
                    .child(div().flex_1().min_w_0().child("Same look for reading and writing")),
            )
            .child(
                div()
                    .flex_none()
                    .id("sync")
                    .child(Switch::new(("sync", 1usize)).checked(on).color(c.link).accessibility_label("Same look for reading and writing").on_click(
                        move |next, window, cx| {
                            let next = *next;
                            view.update(cx, |this, cx| this.set_synced(next, window, cx)).ok();
                        },
                    ))
                    .test_support(),
            )
    }

    /// The reader's paper tones, each a swatch of its page with "Aa" in its ink.
    fn render_tones(&self, c: &Colors, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mode = self.appearance().mode;
        div().flex().gap(px(6.)).children(Mode::TONES.map(|tone| {
            let t = Appearance { mode: tone, ..Appearance::default() }.tokens();
            let on = mode == tone;
            div()
                .id(("mode", tone as usize))
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.))
                .cursor_pointer()
                .child(
                    div()
                        .w_full()
                        .h(px(34.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.))
                        .bg(rgb(t.ground.hex()))
                        .text_color(rgb(t.ink.hex()))
                        .font_family(self.theme().body.face)
                        .text_size(px(15.))
                        .border(if on { px(2.) } else { px(1.) })
                        .border_color(if on { c.link } else { c.hairline })
                        .child("Aa"),
                )
                .child(div().text_size(px(11.)).text_color(if on { c.ink } else { c.muted }).child(tone.label()))
                .on_click(cx.listener(move |this, _, window, cx| this.set_appearance(|a| a.mode = tone, window, cx)))
                .test_support()
        }))
    }

    /// Scroll or Paged.
    fn render_pages(&self, c: &Colors, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let pages = self.settings.reading.pages;
        div().id("pages").flex().gap_1().p_1().bg(c.segment).rounded(px(10.)).children([(PageMode::Scroll, "Scroll"), (PageMode::Paged, "Paged")].into_iter().enumerate().map(|(i, (mode, label))| {
            let on = pages == mode;
            div()
                .id(("pages", i))
                .flex_1()
                .h(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .text_size(px(13.))
                .font_weight(if on { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                .text_color(if on { c.ink } else { c.muted })
                .when(on, |tab| tab.bg(c.surface).border_1().border_color(c.hairline))
                .cursor_pointer()
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| this.set_page_mode(mode, window, cx)))
                .test_support()
        }))
    }

    /// Markdown, then maths. Maths is drawn only with Markdown, so its switch is disabled while
    /// Markdown is off and keeps its saved value for when Markdown comes back. A flip hands focus
    /// back to the page, so the writer sees the change and keeps typing where they were.
    fn render_switches(&self, c: &Colors, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let a = &self.settings.appearance;
        let view = cx.entity().downgrade();
        let row = |id: &'static str, label: &'static str, on: bool, enabled: bool, set: fn(&mut Appearance, bool)| {
            let view = view.clone();
            div()
                .h(px(28.))
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(13.))
                .when(!enabled, |row| row.text_color(c.muted))
                .child(label)
                .child(
                    div()
                        .id(id)
                        .child(
                            Switch::new((id, 1usize))
                                .checked(on)
                                .disabled(!enabled)
                                .color(c.link)
                                .accessibility_label(label)
                                .on_click(move |next, window, cx| {
                                    let next = *next;
                                    view.update(cx, |this, cx| {
                                        this.set_appearance(|a| set(a, next), window, cx);
                                        this.focus_page(window, cx);
                                    })
                                    .ok();
                                }),
                        )
                        .test_support(),
                )
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(row("markdown", "Markdown", a.markdown, true, |a, on| a.markdown = on))
            .child(row("maths", "Maths", a.maths, a.markdown, |a, on| a.maths = on))
    }

    pub(crate) fn render_appearance(&self, panel: &AppearancePanel, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = self.theme().c;
        let a = self.appearance();
        let view = self.view();
        let label = |text: &'static str| {
            div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(c.muted).child(text)
        };
        let section = |title: &'static str| div().flex().flex_col().gap_2().child(label(title));
        let shown = if self.faces.contains(&a.face) { a.face } else { Face::SourceSerif };
        let faces = div().flex().flex_col().children(self.faces.iter().enumerate().map(|(i, &face)| {
            let on = face == shown;
            div()
                .id(("face", i))
                .h(px(34.))
                .px(px(10.))
                .flex()
                .items_center()
                .justify_between()
                .rounded(px(8.))
                .when(on, |row| row.bg(c.segment))
                .hover(|row| row.bg(c.segment))
                .cursor_pointer()
                .child(div().font_family(face.family()).text_size(px(16.)).child(face.family()))
                .when(on, |row| row.child(div().text_size(px(13.)).child("✓")))
                .on_click(cx.listener(move |this, _, window, cx| this.set_appearance(|a| a.face = face, window, cx)))
                .test_support()
        }));
        let sliders = Measure::ALL.into_iter().zip(&panel.sliders).map(|(measure, state)| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .text_size(px(13.))
                        .child(measure.label())
                        .child(div().id((measure.id(), 1usize)).text_color(c.muted).child(measure.show(a)).test_support()),
                )
                .child(div().id(measure.id()).child(Slider::new(state).bg(c.link).text_color(c.surface)).test_support())
        });
        div()
            .id("appearance")
            .w(PANEL_WIDTH)
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(c.surface)
            .border_r_1()
            .border_color(c.hairline)
            .text_color(c.ink)
            .child(
                div()
                    .h(px(56.))
                    .flex_none()
                    .pl(px(56.))
                    .pr(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(Icon::Artboard.draw(px(16.), c.ink))
                    .child(div().flex_1().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child("Appearance"))
                    .child(
                        div()
                            .id("appearance-view")
                            .text_size(px(12.))
                            .text_color(c.muted)
                            .child(match view {
                                View::Writing => "Writing",
                                View::Reading => "Reading",
                            })
                            .test_support(),
                    )
                    .child(
                        ghost_button(&c, "appearance-done", "Done")
                            .text_color(c.ink)
                            .on_click(cx.listener(|this, _, _, cx| this.close_appearance(cx)))
                            .test_support(),
                    ),
            )
            .child(
                div()
                    .id("appearance-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(20.))
                    .pt_2()
                    .pb_6()
                    .flex()
                    .flex_col()
                    .gap_6()
                    .child(self.render_sync(&c, cx))
                    .child(div().flex().flex_col().gap_3().child(self.render_modes(&c, cx)).child(self.render_tones(&c, cx)))
                    .child(section("Font").child(faces))
                    .child(div().flex().flex_col().gap_5().children(sliders))
                    .map(|body| match view {
                        View::Writing => body.child(section("Drawing").child(self.render_switches(&c, cx))),
                        View::Reading => body.child(section("Pages").child(self.render_pages(&c, cx))),
                    })
                    .child(section("Colours").child(self.render_colors(&c, cx)))
                    .child(
                        div().flex().child(
                            ghost_button(&c, "appearance-reset", "Reset")
                                .border_1()
                                .border_color(c.hairline)
                                .text_color(c.ink)
                                .on_click(cx.listener(|this, _, window, cx| this.reset_appearance(window, cx)))
                                .test_support(),
                        ),
                    )
                    .test_support(),
            )
            .test_support()
    }
}
