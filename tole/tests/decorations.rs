//! A window's decorations negotiated with the real compositor: hyprix,
//! headless, in a thread of this test, as on Ferrix.
//!
//! A program says who it wants to draw its frame
//! ([`WindowOptions::decorations`]); the window asks through
//! `xdg-decoration` and [`Window::draws_own_frame`] says what the compositor
//! decided, which is when the program shows its own minimize, maximize and
//! close. One test, in a binary of its own, because the window reaches the
//! compositor through `WAYLAND_DISPLAY`, which is the process's.

#![expect(
    clippy::expect_used,
    reason = "a test's fixtures should fail loudly, and the workspace's ban is about programs"
)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use compositor_toolkit::Event;
use hyprix::{Options, Renderer};
use tole::window::{Decorations, Window, WindowOptions};

/// A directory of this test's own.
fn workspace(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("tole-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a directory to work in");
    path
}

/// What a window asking `decorations` hears under `config`: whether it
/// draws its own frame once configured, and every
/// [`Event::Decorated`] it was handed.
fn negotiated(name: &str, config: &str, decorations: Decorations) -> (bool, Vec<bool>) {
    let work = workspace(name);
    let socket = work.join("wayland");
    let conf = work.join("hyprland.conf");
    std::fs::write(
        &conf,
        format!("animations:enabled = false\ndecoration:blur:noise = 0\n{config}"),
    )
    .expect("a configuration");
    let options = Options {
        display: socket.to_string_lossy().into_owned(),
        headless: Some((640, 480)),
        deadline: Some(2500),
        config: Some(conf),
        renderer: Renderer::Software,
        ..Options::default()
    };
    let path = socket.clone();
    let client = std::thread::spawn(move || window_on(&path, decorations));
    let ran = hyprix::run(&options);
    let answer = client.join().expect("the window's thread finished");
    let _ = ran.expect("the compositor ran");
    let _ = std::fs::remove_dir_all(&work);
    answer
}

/// Open a window on the compositor at `socket` and dispatch until it is
/// configured and a little after.
fn window_on(socket: &Path, decorations: Decorations) -> (bool, Vec<bool>) {
    for _ in 0..400 {
        if socket.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    // SAFETY: this binary's one test sets it before any window connects,
    // and nothing else in the process reads or writes the environment
    // while it does: the compositor's thread reads its socket path from
    // its options.
    unsafe { std::env::set_var("WAYLAND_DISPLAY", socket) };
    let mut ui = tole::Ui::new(tole::Fonts::new());
    let root = ui
        .create(tole::KindName::Rectangle, None)
        .expect("a root item");
    let mut window = Window::open(
        ui.common(root).expect("the root's properties"),
        WindowOptions {
            title: "decorations".to_owned(),
            app_id: "tole-decorations".to_owned(),
            size: (300, 200),
            decorations,
            ..WindowOptions::default()
        },
    )
    .expect("a window on the compositor");
    let mut said = Vec::new();
    let end = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < end {
        let Ok(events) = window.dispatch(&mut ui, Some(Duration::from_millis(50))) else {
            break;
        };
        said.extend(events.iter().filter_map(|event| match event {
            Event::Decorated { own, .. } => Some(*own),
            _ => None,
        }));
    }
    (window.draws_own_frame(), said)
}

/// Where the compositor draws title bars (Yoru's `hyprix:titlebars`), a
/// window asking to draw its own frame is told yes, and one asking for the
/// compositor's (every other window, the default) is told no; a tiling
/// desktop draws none and tells every window the compositor's, so a
/// program shows no buttons of its own in a tile.
#[test]
fn a_window_draws_its_own_frame_only_where_the_compositor_agrees() {
    let bars = "general:layout = floating\nhyprix:titlebars = 1\n";
    assert_eq!(
        negotiated("client", bars, Decorations::Client),
        (true, vec![true]),
        "asked for its own, under title bars"
    );
    assert_eq!(
        negotiated("server", bars, Decorations::Server),
        (false, vec![false]),
        "asked for the compositor's"
    );
    assert_eq!(
        negotiated("tiled", "", Decorations::Client),
        (false, vec![false]),
        "asked for its own, in a tile"
    );
}
