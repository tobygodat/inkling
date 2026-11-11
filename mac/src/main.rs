use gpui_kit::*;
use inkling_ui::{FormFactor, Inkling};

#[cfg(feature = "snap")]
mod snap;

gpui_kit::actions!(inkling_mac, [Quit]);

pub fn window_options(size: Size<Pixels>, focus: bool, show: bool) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds { origin: Default::default(), size })),
        titlebar: Some(TitlebarOptions { title: Some("Inkling".into()), ..Default::default() }),
        focus,
        show,
        ..Default::default()
    }
}

pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    inkling_ui::init(cx);
    cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
    cx.on_action(|_: &Quit, cx| cx.quit());
}

pub fn open(cx: &mut App, form: FormFactor, focus: bool, show: bool) -> Result<(AnyWindowHandle, Entity<Inkling>)> {
    let window_size = match form {
        FormFactor::Mac => size(px(1280.), px(800.)),
        FormFactor::Phone => size(px(390.), px(844.)),
    };
    gpui_kit::open_window(window_options(window_size, focus, show), cx, |window, cx| {
        cx.new(|cx| Inkling::new(form, inkling_core::Paths::mac(), inkling_core::default_keys(), window, cx))
    })
}

fn main() {
    #[cfg(feature = "snap")]
    if let Some(dir) = std::env::args().skip_while(|a| a != "--snap").nth(1) {
        return snap::run(std::path::Path::new(&dir));
    }
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| {
        init(cx);
        cx.set_menus(vec![Menu { name: "Inkling".into(), items: vec![MenuItem::action("Quit Inkling", Quit)], disabled: false }]);
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        open(cx, FormFactor::Mac, true, true).expect("open the window");
        cx.activate(true);
    });
}
