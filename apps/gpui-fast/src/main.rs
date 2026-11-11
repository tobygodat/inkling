use gpui::{AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use inkling_core::{MemoryKeys, Paths, default_keys};
use inkling_gpui_fast::app::{Inkling, init};

fn main() {
    // INKLING_SMOKE=1 opens the window unfocused on the rightmost display (a virtual one in
    // snap.sh), prints when the first frame lands, and quits. It never touches the Keychain.
    let smoke = std::env::var_os("INKLING_SMOKE").is_some();
    gpui_platform::application().run(move |cx| {
        init(cx);
        let display = smoke
            .then(|| cx.displays().into_iter().max_by(|a, b| a.bounds().origin.x.partial_cmp(&b.bounds().origin.x).unwrap()))
            .flatten()
            .map(|d| d.id());
        let bounds = Bounds::centered(display, size(px(900.), px(720.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some("Inkling".into()), ..Default::default() }),
            display_id: display,
            focus: !smoke,
            ..Default::default()
        };
        let window = cx
            .open_window(options, |window, cx| {
                let keys = if smoke { Box::new(MemoryKeys::default()) } else { default_keys() };
                cx.new(|cx| Inkling::new(Paths::mac(), keys, window, cx))
            })
            .expect("open the window");
        if smoke {
            window
                .update(cx, |_, window, _| {
                    window.on_next_frame(|_, cx| {
                        println!("first frame drawn");
                        cx.quit();
                    })
                })
                .expect("the window is open");
        } else {
            cx.activate(true);
        }
    });
}
