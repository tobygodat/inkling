//! SKETCH: changes to mac/src/main.rs. The Mac host gives `ui/` three things for the loop:
//!
//! 1. `Build::running()` at launch: the build number (compiled in from `INKLING_BUILD`, the same
//!    `$build` install.sh writes to CFBundleVersion) and the compiled-in records.
//! 2. `Inkling::set_installed_build(n)` every few seconds: CFBundleVersion of the bundle on disk.
//! 3. A subscription to `inkling_ui::Restart`: relaunch the bundle on disk, then quit.
//!
//! The agent never relaunches the app (decision 10). install.sh replaces the bundle under the
//! running app; this host notices, the view offers Restart, and only the writer's press restarts.

use gpui_kit::*;
use inkling_core::{Build, BuildNumber, KeyStore};
use inkling_ui::{FormFactor, Inkling, Restart};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often to look at the bundle on disk. Reading a 1 KB plist; costs nothing.
const WATCH: Duration = Duration::from_secs(5);

/// `keys` and `build` are parameters now, so the snap can pass a store that outlives a relaunch
/// and a build with records. `open` itself never watches the bundle or restarts: only `main`
/// wires those, so a snap can't quit the process it runs in.
pub fn open(
    cx: &mut App,
    form: FormFactor,
    focus: bool,
    show: bool,
    keys: Box<dyn KeyStore>,
    build: Build,
) -> Result<(AnyWindowHandle, Entity<Inkling>)> {
    todo!("as today, passing keys and build to Inkling::new")
}

fn main() {
    // … snap branch as today …
    // gpui_kit::application()….run(|cx| {
    //     init(cx); menus; on_window_closed → quit;
    //     let (_, view) = open(cx, FormFactor::Mac, true, true, inkling_core::default_keys(), Build::running())?;
    //     if let Some(bundle) = running_bundle() {
    //         watch_bundle(&view, bundle.clone(), cx);
    //         cx.subscribe(&view, move |_, _: &Restart, cx| restart(&bundle, cx)).detach();
    //     }
    //     cx.activate(true);
    // });
    todo!()
}

/// The `.app` this process was launched from, or `None` under `cargo run`.
/// `current_exe()` is `<bundle>/Contents/MacOS/Inkling`; its path stays valid after install.sh
/// replaces the bundle, and then points into the new one.
fn running_bundle() -> Option<PathBuf> {
    todo!("current_exe().ancestors().find(|p| p.extension() == Some(\"app\"))")
}

/// CFBundleVersion of `<bundle>/Contents/Info.plist`, or `None` mid-install (no file yet) or
/// for anything that isn't a whole number. install.sh writes the plist, so its exact layout is
/// ours: `<key>CFBundleVersion</key><string>N</string>`. No plist crate.
fn bundle_build(bundle: &Path) -> Option<BuildNumber> {
    todo!()
}

fn watch_bundle(view: &Entity<Inkling>, bundle: PathBuf, cx: &mut App) {
    // TODO: cx.spawn loop { timer(WATCH).await; let n = bundle_build(&bundle);
    //        view.update(cx, |v, cx| v.set_installed_build(n, cx)) } until the view is gone.
    todo!()
}

/// The view has already saved the draft. Starts a detached `sh` that waits for this pid to exit
/// and then `open`s the bundle, and quits. Waiting first means the old and new process never run
/// side by side writing the same draft file.
fn restart(bundle: &Path, cx: &mut App) {
    // TODO:
    // Command::new("/bin/sh").args(["-c", r#"while kill -0 "$0" 2>/dev/null; do sleep 0.1; done; exec /usr/bin/open "$1""#,
    //                               &std::process::id().to_string(), bundle.to_str()?]).spawn();
    // On spawn error: leave the app running (the writer can quit and open it by hand).
    // cx.quit();
    todo!()
}
