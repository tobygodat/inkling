//! Formulas drawn by RaTeX inside GPUI Kit's text, with no web view: TeX is laid out, written as
//! SVG with its glyphs embedded, and rasterised at the window's scale. A formula RaTeX can't
//! parse is never claimed, so the kit draws its source as text.

use gpui_kit::base::text::{InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast};
use gpui_kit::*;
use inkling_core::draft::{Claim, draws_inline_maths};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const INLINE: &str = "maths-inline";
const BLOCK: &str = "maths-block";

#[derive(Clone)]
struct Tex {
    source: String,
    display: bool,
}

struct Formula {
    image: Arc<RenderImage>,
    width: f32,
    height: f32,
    depth: f32,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    tex: String,
    display: bool,
    size: u32,
    scale: u32,
    ink: u32,
}

/// Parsed and drawn formulas, kept for the editor's life, and where each was drawn last frame.
#[derive(Default)]
pub(crate) struct Maths {
    parses: Mutex<HashMap<String, bool>>,
    drawn: Mutex<HashMap<Key, Option<Arc<Formula>>>>,
    pub(crate) painted: Mutex<Vec<(String, Bounds<Pixels>)>>,
}

impl Maths {
    /// Claims a formula only when RaTeX can parse it.
    pub fn claim(self: &Arc<Self>) -> Claim {
        let maths = self.clone();
        Arc::new(move |tex: &str, _| {
            let mut parses = maths.parses.lock().unwrap_or_else(|e| e.into_inner());
            *parses.entry(tex.to_string()).or_insert_with(|| ratex_parser::parser::parse(tex).is_ok())
        })
    }

    fn formula(&self, tex: &Tex, size: Pixels, scale: f32, ink: Rgba, cx: &App) -> Option<Arc<Formula>> {
        let ink_hex = (ink.r * 255.).round() as u32 * 65536 + (ink.g * 255.).round() as u32 * 256 + (ink.b * 255.).round() as u32;
        let key = Key { tex: tex.source.clone(), display: tex.display, size: size.as_f32().to_bits(), scale: scale.to_bits(), ink: ink_hex };
        let mut drawn = self.drawn.lock().unwrap_or_else(|e| e.into_inner());
        drawn
            .entry(key)
            .or_insert_with(|| {
                let ast = ratex_parser::parser::parse(&tex.source).ok()?;
                let style = if tex.display { ratex_types::math_style::MathStyle::Display } else { ratex_types::math_style::MathStyle::Text };
                let color = ratex_types::color::Color { r: ink.r, g: ink.g, b: ink.b, a: 1. };
                let options = ratex_layout::LayoutOptions::default().with_style(style).with_color(color);
                let list = ratex_layout::to_display_list(&ratex_layout::layout(&ast, &options));
                let svg = ratex_svg::render_to_svg(
                    &list,
                    &ratex_svg::SvgOptions { font_size: size.as_f32().into(), padding: 0., stroke_width: 1., embed_glyphs: true, font_dir: String::new() },
                );
                let image = cx.svg_renderer().render_single_frame(svg.as_bytes(), scale).ok()?;
                Some(Arc::new(Formula { image, width: list.width as f32, height: list.height as f32, depth: list.depth as f32 }))
            })
            .clone()
    }

    /// Layout rounds an object's height, and GPUI Kit drops an inline object whose baseline sits
    /// below its height, so the height is rounded up: a formula with no depth would lose its
    /// image otherwise.
    fn image(self: &Arc<Self>, tex: &Tex, f: &Formula, size: Pixels) -> Div {
        let maths = self.clone();
        let source = tex.source.clone();
        div()
            .on_children_prepainted(move |bounds, _, _| {
                if let Some(b) = bounds.first() {
                    maths.painted.lock().unwrap_or_else(|e| e.into_inner()).push((source.clone(), *b));
                }
            })
            .child(img(f.image.clone()).w(size * f.width).h((size * (f.height + f.depth)).ceil()))
    }
}

/// Inline `$…$` or display `$$…$$`, drawn in `ink` at the body `size`.
pub(crate) struct MathsPlugin {
    pub maths: Arc<Maths>,
    pub claim: Claim,
    pub display: bool,
    pub size: Pixels,
    pub ink: Rgba,
}

impl MarkdownPlugin for MathsPlugin {
    fn is_block(&self) -> bool {
        self.display
    }

    fn name(&self) -> &str {
        if self.display { BLOCK } else { INLINE }
    }

    fn parse(&self, node: &markdown_ast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let claimed = match node {
            markdown_ast::Node::InlineMath(m) if !self.display => {
                let p = node.position()?;
                draws_inline_maths(cx.source(), p.start.offset..p.end.offset, &m.value, &self.claim).then_some(&m.value)
            }
            markdown_ast::Node::Math(m) if self.display => (self.claim)(&m.value, true).then_some(&m.value),
            _ => None,
        }?;
        let raw = cx.node_source(node)?.to_string();
        Some(
            MarkdownNode::new(self.name().to_string(), Tex { source: claimed.clone(), display: self.display })
                .accessibility_label(format!("Formula {claimed}"))
                .text(raw.clone())
                .markdown(raw),
        )
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let block = div().w_full().flex().justify_center();
        let drawn = node.data::<Tex>().and_then(|tex| Some((tex, self.maths.formula(tex, self.size, window.scale_factor(), self.ink, cx)?)));
        match drawn {
            Some((tex, f)) => block
                .id(SharedString::from(format!("formula {}", tex.source)))
                .role(Role::Image)
                .aria_label(format!("Formula {}", tex.source))
                .child(self.maths.image(tex, &f, self.size))
                .into_any_element(),
            None => block.child(node.as_text().to_string()).into_any_element(),
        }
    }

    fn render_inline(&self, node: &MarkdownNode, context: &InlineRenderContext, window: &mut Window, cx: &mut App) -> Option<InlineElement> {
        let tex = node.data::<Tex>()?;
        let size = context.font_size();
        let f = self.maths.formula(tex, size, window.scale_factor(), self.ink, cx)?;
        Some(InlineElement::new(self.maths.image(tex, &f, size)).with_baseline(size * f.height))
    }
}

pub(crate) const REMOTE_IMAGE: &str = "remote-image";

/// A remote image is drawn as its alt text and never fetched. The editor puts these nodes in
/// place of remote images; the plugin claims nothing from the parser.
pub(crate) struct RemoteImage {
    pub muted: Rgba,
}

impl MarkdownPlugin for RemoteImage {
    fn name(&self) -> &str {
        REMOTE_IMAGE
    }

    fn parse(&self, _: &markdown_ast::Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        None
    }

    fn render_inline(&self, node: &MarkdownNode, _: &InlineRenderContext, _: &mut Window, _: &mut App) -> Option<InlineElement> {
        Some(InlineElement::new(div().italic().text_color(self.muted).child(node.as_text().to_string())))
    }
}
