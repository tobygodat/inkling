//! The Appearance panel: beside the page on the Mac, so every choice shows on the page at once.
//! Each change goes through `Inkling::set_appearance`, which saves it and recolours the kit.

use crate::app::{Inkling, Screen, ghost_button};
use crate::icons::Icon;
use crate::picker::Picker;
use crate::theme::Colors;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_core::appearance::{LineSpacing, PageWidth, TextSize};
use inkling_core::{Appearance, Face};

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

/// Open while `Inkling::screen` is `Screen::Page(Some(_))`.
pub(crate) struct AppearancePanel {
    sliders: [Entity<SliderState>; 3],
    /// The colour row that is open, if any.
    pub picker: Option<Picker>,
    _subs: Vec<Subscription>,
}

impl AppearancePanel {
    fn new(appearance: &Appearance, window: &mut Window, cx: &mut Context<Inkling>) -> Self {
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
        AppearancePanel { sliders, picker: None, _subs: subs }
    }

    /// Moves each slider to what `appearance` holds, after a change that didn't come from it.
    fn follow(&self, appearance: &Appearance, window: &mut Window, cx: &mut App) {
        for (slider, measure) in self.sliders.iter().zip(Measure::ALL) {
            slider.update(cx, |s, cx| s.set_value(f32::from(measure.get(appearance)), window, cx));
        }
    }
}

impl Inkling {
    /// The Appearance row of the cog's menu. The panel opens beside the page, and the page stays.
    pub fn open_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu(cx);
        if matches!(self.screen, Screen::Page(Some(_))) {
            return;
        }
        if self.screen.keys().is_some() || self.screen.feedback().is_some() {
            self.screen = Screen::Page(None);
            self.focus_page(window, cx);
        }
        let panel = AppearancePanel::new(&self.settings.appearance, window, cx);
        self.screen = Screen::Page(Some(panel));
        cx.notify();
    }

    pub fn close_appearance(&mut self, cx: &mut Context<Self>) {
        if let Screen::Page(panel) = &mut self.screen {
            *panel = None;
            cx.notify();
        }
    }

    pub fn appearance(&self) -> &Appearance {
        &self.settings.appearance
    }

    /// The body faces this device has, in the panel's order.
    pub fn faces(&self) -> &[Face] {
        &self.faces
    }

    pub fn appearance_open(&self) -> bool {
        matches!(self.screen, Screen::Page(Some(_)))
    }

    /// Changes the appearance, draws it everywhere at once and saves it.
    pub fn set_appearance(&mut self, change: impl FnOnce(&mut Appearance), window: &mut Window, cx: &mut Context<Self>) {
        let before = self.settings.appearance.clone();
        change(&mut self.settings.appearance);
        if self.settings.appearance == before {
            return;
        }
        if let Err(e) = self.settings.save(&self.paths) {
            self.notice = Some(format!("Couldn't save the appearance ({e}).").into());
        }
        let theme = self.theme();
        theme.apply_to_kit(cx);
        self.page.set_look(&self.settings.appearance, theme, cx);
        window.refresh();
        cx.notify();
    }

    /// Reset: the default Light look, with every slider back where it started.
    pub fn reset_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_picker(cx);
        self.set_appearance(|a| *a = Appearance::default(), window, cx);
        if let Screen::Page(Some(panel)) = &self.screen {
            panel.follow(&self.settings.appearance, window, cx);
        }
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
        let a = &self.settings.appearance;
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
                    .child(self.render_modes(&c, cx))
                    .child(section("Font").child(faces))
                    .child(div().flex().flex_col().gap_5().children(sliders))
                    .child(section("Drawing").child(self.render_switches(&c, cx)))
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
