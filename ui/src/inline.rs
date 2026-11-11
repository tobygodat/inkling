//! Controls that sit inside running text.
//!
//! GPUI's text has no inline elements, so the text reserves room for each control with a run of
//! no-break spaces, which also keeps the control on the line of the word before it. After the
//! text is laid out, each control is laid out on its own and placed at its run's position,
//! centred on the line. The controls are ordinary elements, so their clicks work by mouse and by
//! touch alike.

use gpui_kit::*;

/// The character a control's room is made of. It never starts a line: UAX #14 glues it to the
/// word before.
pub const ROOM: char = '\u{a0}';

pub struct InlineText {
    text: StyledText,
    /// Each control and the byte index of the room it sits in.
    controls: Vec<(usize, AnyElement)>,
}

impl InlineText {
    pub fn new(text: StyledText, controls: Vec<(usize, AnyElement)>) -> Self {
        InlineText { text, controls }
    }
}

/// The width `text` takes when shaped in `font` at `size`.
pub fn width_of(text: &str, font: &Font, size: Pixels, window: &Window) -> Pixels {
    let mut run = window.text_style().to_run(text.len());
    run.font = font.clone();
    window.text_system().shape_line(text.to_string().into(), size, &[run], None).width
}

/// How many `ROOM` characters cover `width` in `font` at `size`. Measured by shaping a run of
/// them, since the shaped width differs from the glyph's nominal advance.
pub fn room_for(width: Pixels, font: &Font, size: Pixels, window: &Window) -> usize {
    const SAMPLE: usize = 16;
    let sample: String = std::iter::repeat_n(ROOM, SAMPLE).collect();
    let each = width_of(&sample, font, size, window) / SAMPLE as f32;
    if each <= px(0.) {
        return 1;
    }
    (width / each).ceil().max(1.) as usize
}

impl IntoElement for InlineText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for InlineText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        Element::request_layout(&mut self.text, id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        Element::prepaint(&mut self.text, id, inspector_id, bounds, state, window, cx);
        let layout = self.text.layout().clone();
        let line = layout.line_height();
        for (at, control) in &mut self.controls {
            let Some(origin) = layout.position_for_index(*at) else { continue };
            let size = control.layout_as_root(AvailableSpace::min_size(), window, cx);
            let top = origin.y + (line - size.height) / 2.;
            control.prepaint_at(point(origin.x, top), window, cx);
        }
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        Element::paint(&mut self.text, id, inspector_id, bounds, state, prepaint, window, cx);
        for (_, control) in &mut self.controls {
            control.paint(window, cx);
        }
    }
}
