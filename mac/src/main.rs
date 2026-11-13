use gpui_kit::*;
use inkling_core::{Build, BuildNumber, KeyStore};
use inkling_ui::{BookHost, FormFactor, Inkling, Restart};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(feature = "snap")]
mod snap;
#[cfg(feature = "snap")]
mod snap_live;

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

/// How often to look at the bundle on disk for a newer build.
const WATCH: Duration = Duration::from_secs(5);

/// Opens the app's window. It never watches the bundle or restarts: only `main` wires those, so
/// a snap can't quit the process it runs in.
pub fn open(
    cx: &mut App,
    form: FormFactor,
    focus: bool,
    show: bool,
    keys: Box<dyn KeyStore>,
    build: Build,
) -> Result<(AnyWindowHandle, Entity<Inkling>)> {
    let window_size = match form {
        FormFactor::Mac => size(px(1280.), px(800.)),
        FormFactor::Phone => size(px(390.), px(844.)),
    };
    gpui_kit::open_window(window_options(window_size, focus, show), cx, |window, cx| {
        cx.new(|cx| Inkling::new(form, inkling_core::Paths::mac(), keys, build, window, cx))
    })
}

fn main() {
    #[cfg(feature = "snap")]
    if let Some(dir) = std::env::args().skip_while(|a| a != "--snap").nth(1) {
        return snap::run(std::path::Path::new(&dir));
    }
    #[cfg(feature = "snap")]
    if let Some(dir) = std::env::args().skip_while(|a| a != "--snap-live").nth(1) {
        return snap_live::run(std::path::Path::new(&dir));
    }
    #[cfg(feature = "snap")]
    if let Some(dir) = std::env::args().skip_while(|a| a != "--snap-books").nth(1) {
        return snap::books::run(std::path::Path::new(&dir));
    }
    #[cfg(feature = "snap")]
    if let Some(dir) = std::env::args().skip_while(|a| a != "--snap-a11y").nth(1) {
        return snap::a11y::run(std::path::Path::new(&dir));
    }
    #[cfg(feature = "snap")]
    if let Some(page) = std::env::args().skip_while(|a| a != "--bench-typing").nth(1) {
        return snap_live::bench(&page);
    }
    gpui_kit::application().with_assets(inkling_ui::Assets).run(|cx| {
        init(cx);
        cx.set_menus(vec![Menu { name: "Inkling".into(), items: vec![MenuItem::action("Quit Inkling", Quit)], disabled: false }]);
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        let (_, view) = open(cx, FormFactor::Mac, true, true, inkling_core::default_keys(), Build::running())
            .expect("open the window");
        view.update(cx, |v, cx| v.set_book_host(book_host(), cx));
        if let Some(bundle) = running_bundle() {
            watch_bundle(&view, bundle.clone(), cx);
            cx.subscribe(&view, move |_, _: &Restart, cx| restart(&bundle, cx)).detach();
        }
        cx.activate(true);
    });
}

/// The Mac's side of the library: the open panel for Add, and the folders Find books searches.
fn book_host() -> BookHost {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    BookHost {
        folders: ["Desktop", "Documents", "Downloads"].map(|f| home.join(f)).to_vec(),
        pick: Box::new(|cx: &mut App| {
            let chosen = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: true,
                prompt: Some("Add".into()),
            });
            cx.spawn(async move |_| chosen.await.ok().and_then(Result::ok).flatten().unwrap_or_default())
        }),
    }
}

/// The `.app` this process was launched from, or `None` under `cargo run`. The path stays valid
/// after `install.sh` swaps the bundle, and then names the new one.
fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
}

/// CFBundleVersion of the bundle's Info.plist, or `None` mid-install or for anything that isn't a
/// whole number. `mac/install.sh` writes the plist, so its layout is known: the key and its
/// string on one line. No plist parser.
fn bundle_build(bundle: &Path) -> Option<BuildNumber> {
    const KEY: &str = "<key>CFBundleVersion</key><string>";
    let plist = std::fs::read_to_string(bundle.join("Contents/Info.plist")).ok()?;
    let rest = &plist[plist.find(KEY)? + KEY.len()..];
    rest[..rest.find("</string>")?].parse().ok().map(BuildNumber)
}

fn watch_bundle(view: &Entity<Inkling>, bundle: PathBuf, cx: &mut App) {
    let view = view.downgrade();
    cx.spawn(async move |cx| {
        loop {
            let installed = bundle_build(&bundle);
            if view.update(cx, |v, cx| v.set_installed_build(installed, cx)).is_err() {
                break;
            }
            cx.background_executor().timer(WATCH).await;
        }
    })
    .detach();
}

/// The view has saved the draft. A detached `sh` waits for this process to exit and then opens
/// the bundle, so the old and new app never run side by side writing the same draft. If it can't
/// start, the app keeps running and the writer can reopen it by hand.
fn restart(bundle: &Path, cx: &mut App) {
    let relaunch = std::process::Command::new("/bin/sh")
        .args(["-c", r#"while kill -0 "$0" 2>/dev/null; do sleep 0.1; done; exec /usr/bin/open "$1""#])
        .arg(std::process::id().to_string())
        .arg(bundle)
        .spawn();
    if relaunch.is_ok() {
        cx.quit();
    }
}
