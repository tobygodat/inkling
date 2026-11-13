//! The Appearance panel's colours: Light, Dark or Custom, one row per colour, and the picker a row
//! opens. The picker shows ten swatches first; Custom adds a colour wheel (angle is hue, distance
//! from the centre is strength), a lightness slider and a hex field.

use crate::app::Inkling;
use crate::theme::Colors;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use inkling_core::{Mode, Part, Rgb};
use std::f32::consts::TAU;

pub(crate) const WHEEL: Pixels = px(200.);
/// The wheel is drawn as squares this size. Smaller is smoother and costs more quads per frame.
const CELL: f32 = 1.5;

/// Ten colours per part that read well in that part's place.
fn swatches(part: Part) -> [u32; 10] {
    match part {
        Part::Background => [
            0xFBFBF9, 0xF6F1E7, 0xF2F4F7, 0xEEF3EC, 0xFFF8F0, 0x15171A, 0x1B1F2A, 0x221F1B, 0x13201C, 0x0B0B0C,
        ],
        Part::Foreground => [
            0xFFFFFF, 0xFBF7EE, 0xF7F8FA, 0xF4F8F2, 0xFFFCF7, 0x1E2125, 0x252A37, 0x2C2924, 0x1A2A25, 0x161618,
        ],
        Part::Text => [
            0x1F2328, 0x2B2B2B, 0x3B3226, 0x1C2B3C, 0x233127, 0xE8E6E1, 0xF2EEE6, 0xDCE4EC, 0xD7E6DA, 0xFFFFFF,
        ],
        Part::Accent => [
            0x0969DA, 0x8250DF, 0xBF3989, 0xBC4C00, 0x9A6700, 0x0E7C86, 0x57606A, 0xD97757, 0x58A6FF, 0xE5534B,
        ],
    }
}

/// The colour row that is open, under that row.
pub(crate) struct Picker {
    pub part: Part,
    pub custom: Option<Custom>,
}

/// The wheel, the lightness slider and the hex field. Hue and strength live here rather than
/// being read back from the colour, which loses its hue at black, white and grey.
pub(crate) struct Custom {
    hue: f32,
    strength: f32,
    lightness: Entity<SliderState>,
    pub hex: Entity<InputState>,
    /// Whether a press on the wheel is still held, so a drag past its edge keeps steering it.
    dragging: bool,
    _subs: [Subscription; 2],
}

impl Custom {
    fn new(color: Rgb, window: &mut Window, cx: &mut Context<Inkling>) -> Self {
        let (hue, strength, lightness) = color.hsl();
        let slider = cx.new(|_| SliderState::new().min(0.).max(100.).step(1.).default_value((lightness * 100.).round()));
        let hex = cx.new(|cx| {
            let mut field = InputState::new(window, cx);
            field.set_value(color.to_string(), window, cx);
            field
        });
        let lit = cx.subscribe_in(&slider, window, |this, _, event: &SliderEvent, window, cx| {
            if let SliderEvent::Change(_) = event {
                this.paint_custom(window, cx);
            }
        });
        let typed = cx.subscribe_in(&hex, window, |this, field, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. })
                && let Some(color) = Rgb::parse(&field.read(cx).value())
            {
                this.pick(color, window, cx);
            }
        });
        Custom { hue, strength, lightness: slider, hex, dragging: false, _subs: [lit, typed] }
    }

    fn color(&self, cx: &App) -> Rgb {
        Rgb::from_hsl(self.hue, self.strength, self.lightness.read(cx).value().start() / 100.)
    }

    /// Moves every control to `color`. The hex field is left alone while it is being typed in.
    fn follow(&mut self, color: Rgb, window: &mut Window, cx: &mut App) {
        let (hue, strength, lightness) = color.hsl();
        if strength > 0. {
            self.hue = hue;
        }
        self.strength = strength;
        self.lightness.update(cx, |s, cx| s.set_value((lightness * 100.).round(), window, cx));
        if !self.hex.read(cx).focus_handle(cx).is_focused(window) {
            self.hex.update(cx, |field, cx| field.set_value(color.to_string(), window, cx));
        }
    }
}

impl Inkling {
    fn picker_mut(&mut self) -> Option<&mut Picker> {
        self.panel.as_mut()?.picker.as_mut()
    }

    fn picker(&self) -> Option<&Picker> {
        self.panel.as_ref()?.picker.as_ref()
    }

    /// Opens the picker under `part`'s row, or closes it when it is already open there.
    pub fn toggle_picker(&mut self, part: Part, cx: &mut Context<Self>) {
        let Some(panel) = &mut self.panel else { return };
        panel.picker = match &panel.picker {
            Some(open) if open.part == part => None,
            _ => Some(Picker { part, custom: None }),
        };
        cx.notify();
    }

    pub(crate) fn close_picker(&mut self, cx: &mut Context<Self>) {
        if let Some(panel) = &mut self.panel {
            panel.picker = None;
            cx.notify();
        }
    }

    /// What the hex field holds, while Custom is open.
    pub fn hex_field(&self, cx: &App) -> Option<String> {
        Some(self.picker()?.custom.as_ref()?.hex.read(cx).value().to_string())
    }

    pub fn picker_part(&self) -> Option<Part> {
        self.picker().map(|p| p.part)
    }

    /// Custom, under the swatches: shows the wheel, the lightness slider and the hex field.
    pub fn open_custom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(part) = self.picker_part() else { return };
        let color = self.appearance().palette().get(part);
        let custom = Custom::new(color, window, cx);
        if let Some(picker) = self.picker_mut() {
            picker.custom = Some(custom);
        }
        cx.notify();
    }

    /// The hex field has focus, so Return and Tab belong to it.
    pub(crate) fn typing_hex(&self, window: &Window, cx: &App) -> bool {
        self.picker()
            .and_then(|p| p.custom.as_ref())
            .is_some_and(|c| c.hex.read(cx).focus_handle(cx).is_focused(window))
    }

    /// A swatch or a hex code: the colour goes in, and the wheel and slider move to it.
    pub fn pick(&mut self, color: Rgb, window: &mut Window, cx: &mut Context<Self>) {
        let Some(part) = self.picker_part() else { return };
        self.set_appearance(|a| a.paint(part, color), window, cx);
        if let Some(custom) = self.picker_mut().and_then(|p| p.custom.as_mut()) {
            custom.follow(color, window, cx);
        }
    }

    /// The wheel or the lightness slider moved.
    fn paint_custom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.picker() else { return };
        let part = picker.part;
        let Some(custom) = &picker.custom else { return };
        let color = custom.color(cx);
        self.set_appearance(|a| a.paint(part, color), window, cx);
        if let Some(custom) = self.picker_mut().and_then(|p| p.custom.as_mut())
            && !custom.hex.read(cx).focus_handle(cx).is_focused(window)
        {
            custom.hex.update(cx, |field, cx| field.set_value(color.to_string(), window, cx));
        }
    }

    /// A press or drag on the wheel at `offset` from its centre. Past the edge it holds the edge.
    fn steer_wheel(&mut self, offset: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(custom) = self.picker_mut().and_then(|p| p.custom.as_mut()) else { return };
        let (x, y) = (f32::from(offset.x), -f32::from(offset.y));
        custom.hue = y.atan2(x).rem_euclid(TAU).to_degrees();
        custom.strength = ((x * x + y * y).sqrt() / (f32::from(WHEEL) / 2.)).min(1.);
        self.paint_custom(window, cx);
    }

    pub(crate) fn render_modes(&self, c: &Colors, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mode = self.appearance().mode;
        div().id("mode").flex().gap_1().p_1().bg(c.segment).rounded(px(10.)).children(Mode::ALL.map(|m| {
            let on = m == mode;
            div()
                .id(("mode", m as usize))
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
                .child(m.label())
                .on_click(cx.listener(move |this, _, window, cx| this.set_appearance(|a| a.mode = m, window, cx)))
                .test_support()
        }))
    }

    pub(crate) fn render_colors(&self, c: &Colors, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let palette = self.appearance().palette();
        let open = self.picker();
        div().flex().flex_col().gap_1().children(Part::ALL.into_iter().enumerate().map(|(i, part)| {
            let color = palette.get(part);
            let row = div()
                .id(("color", i))
                .h(px(36.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(8.))
                .when(open.is_some_and(|p| p.part == part), |row| row.bg(c.segment))
                .hover(|row| row.bg(c.segment))
                .cursor_pointer()
                .child(swatch(color, px(20.), c))
                .child(div().flex_1().text_size(px(13.)).child(part.label()))
                .child(div().text_size(px(12.)).text_color(c.muted).child(color.to_string()))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_picker(part, cx)))
                .test_support();
            let picker = open.filter(|p| p.part == part).map(|p| self.render_picker(p, color, c, cx));
            div().flex().flex_col().child(row).children(picker)
        }))
    }

    fn render_picker(&self, picker: &Picker, current: Rgb, c: &Colors, cx: &mut Context<Self>) -> AnyElement {
        let swatches = div().flex().flex_wrap().gap(px(10.)).children(swatches(picker.part).into_iter().enumerate().map(|(i, hex)| {
            let color = Rgb::new(hex);
            div()
                .id(("swatch", i))
                .p(px(2.))
                .rounded_full()
                .border_2()
                .border_color(if color == current { c.ink } else { transparent_black().into() })
                .cursor_pointer()
                .child(swatch(color, px(28.), c))
                .on_click(cx.listener(move |this, _, window, cx| this.pick(color, window, cx)))
                .test_support()
        }));
        let custom = match &picker.custom {
            None => div()
                .id("custom")
                .h(px(30.))
                .px_3()
                .flex()
                .items_center()
                .rounded(px(8.))
                .border_1()
                .border_color(c.hairline)
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .child("Custom")
                .on_click(cx.listener(|this, _, window, cx| this.open_custom(window, cx)))
                .test_support()
                .into_any_element(),
            Some(custom) => div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .child(div().id("wheel").child(self.render_wheel(custom, c.surface, cx)).test_support())
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(div().text_size(px(13.)).child("Lightness"))
                        .child(div().id("lightness").child(Slider::new(&custom.lightness).bg(c.link).text_color(c.surface)).test_support()),
                )
                .child(
                    div()
                        .id("hex")
                        .w_full()
                        .h(px(34.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .bg(c.ground)
                        .border_1()
                        .border_color(c.hairline)
                        .rounded(px(8.))
                        .text_size(px(13.))
                        .child(div().text_color(c.muted).child("Hex"))
                        .child(div().flex_1().child(Input::new(&custom.hex).id("hex-field").appearance(false)))
                        .test_support(),
                )
                .into_any_element(),
        };
        div().pt_2().pb_3().px(px(10.)).flex().flex_col().gap_3().child(swatches).child(div().w_full().flex().justify_center().child(custom)).into_any_element()
    }

    /// The wheel at the custom colour's lightness, with a ring on the colour. It is a grid of
    /// small squares painted each frame; the press and drag listeners are registered in paint,
    /// where the wheel knows its bounds.
    fn render_wheel(&self, custom: &Custom, around: Rgba, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let lightness = custom.lightness.read(cx).value().start() / 100.;
        let (hue, strength) = (custom.hue, custom.strength);
        let view = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let r = f32::from(bounds.size.width) / 2.;
                let center = bounds.center();
                let n = (2. * r / CELL).ceil() as i32;
                for iy in 0..n {
                    for ix in 0..n {
                        let (x, y) = ((ix as f32 + 0.5) * CELL - r, r - (iy as f32 + 0.5) * CELL);
                        let d = (x * x + y * y).sqrt();
                        if d > r {
                            continue;
                        }
                        let color = Rgb::from_hsl(y.atan2(x).rem_euclid(TAU).to_degrees(), d / r, lightness);
                        let origin = point(bounds.left() + px(ix as f32 * CELL), bounds.top() + px(iy as f32 * CELL));
                        window.paint_quad(fill(Bounds::new(origin, size(px(CELL), px(CELL))), rgb(color.hex())));
                    }
                }
                // Squares leave a stepped rim; an anti-aliased ring in the panel's colour hides it.
                let rim = Bounds::centered_at(center, size(px(2. * r + 4.), px(2. * r + 4.)));
                window.paint_quad(quad(rim, px(r + 2.), transparent_black(), px(3.), around, BorderStyle::Solid));
                let at = hue.to_radians();
                let dot = point(center.x + px(at.cos() * strength * r), center.y - px(at.sin() * strength * r));
                let ring = Bounds::centered_at(dot, size(px(14.), px(14.)));
                window.paint_quad(quad(ring, px(7.), transparent_black(), px(2.), white(), BorderStyle::Solid));

                let pressed = view.clone();
                window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                    let inside = (e.position - center).magnitude() <= f64::from(r);
                    if phase.bubble() && e.button == MouseButton::Left && inside {
                        let _ = pressed.update(cx, |this, cx| {
                            this.wheel_dragging(true);
                            this.steer_wheel(e.position - center, window, cx);
                        });
                    }
                });
                let moved = view.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                    if phase.bubble() && e.pressed_button == Some(MouseButton::Left) {
                        let _ = moved.update(cx, |this, cx| {
                            if this.is_wheel_dragging() {
                                this.steer_wheel(e.position - center, window, cx);
                            }
                        });
                    }
                });
                let released = view.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase.bubble() {
                        let _ = released.update(cx, |this, _| this.wheel_dragging(false));
                    }
                });
            },
        )
        .size(WHEEL)
        .flex_none()
    }

    fn wheel_dragging(&mut self, on: bool) {
        if let Some(custom) = self.picker_mut().and_then(|p| p.custom.as_mut()) {
            custom.dragging = on;
        }
    }

    fn is_wheel_dragging(&self) -> bool {
        self.picker().and_then(|p| p.custom.as_ref()).is_some_and(|c| c.dragging)
    }
}

fn swatch(color: Rgb, size: Pixels, c: &Colors) -> Div {
    div().size(size).flex_none().rounded_full().bg(rgb(color.hex())).border_1().border_color(c.hairline)
}
