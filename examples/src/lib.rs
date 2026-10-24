//! tole's demos: each a small `.tole` program under `ui/`, compiled by
//! `build.rs`, and drawn here headless at its own size. `tole-examples
//! --all docs/screenshots` writes the README's screenshots; the tests fail
//! when one of them no longer matches what the demo draws.

use std::path::PathBuf;

use tole::{Color, Element, Fonts, Ui};

tole::include_ui!();

/// A demo: its name, and how to make it in a `Ui`.
#[derive(Clone, Copy, Debug)]
pub struct Demo {
    /// Its name: the screenshot's file name.
    pub name: &'static str,
    /// What it shows.
    pub about: &'static str,
    /// Make it, as a root, and do to it what its screenshot shows;
    /// answers its root's common properties.
    pub make: fn(&mut Ui) -> Option<tole::Common>,
}

/// Press and release the pointer on the item whose text is `label`, or
/// only move over it when `click` is false: how a demo's script works its
/// buttons without knowing where they are.
pub fn press(ui: &mut Ui, label: &str, click: bool) -> Option<()> {
    let item = find_text(ui, label)?;
    let rect = ui.rect_of(item)?;
    ui.pointer_move(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    if click {
        ui.pointer_press();
        ui.pointer_release();
    }
    Some(())
}

/// Press and release the key xkb names `key` (`"Tab"`, `"space"`,
/// `"Right"`), as a window hands it over.
pub fn key(ui: &mut Ui, key: &str) {
    let event = tole::KeyEvent::named(key);
    let _ = ui.key_press(&event);
    let _ = ui.key_release(&event);
}

/// The first `Text` item, anywhere, saying `label`.
pub fn find_text(ui: &mut Ui, label: &str) -> Option<tole::ItemId> {
    let mut stack: Vec<tole::ItemId> = ui.roots().to_vec();
    while let Some(item) = stack.pop() {
        if let Some(text) = ui.text(item)
            && &*ui.get(text.text) == label
        {
            return Some(item);
        }
        stack.extend(ui.children(item).iter().rev());
    }
    None
}

/// Every demo, in the README's order.
pub const DEMOS: [Demo; 13] = [
    Demo {
        name: "hello",
        about: "the smallest program",
        make: |ui| Hello::new(ui, None).map(|root| root.common()),
    },
    Demo {
        name: "layouts",
        about: "anchors, a Row and a Column",
        make: |ui| Layouts::new(ui, None).map(|root| root.common()),
    },
    Demo {
        name: "counter",
        about: "signals and handlers: three clicks, and the pointer over Reset",
        make: |ui| {
            let root = Counter::new(ui, None)?;
            for _ in 0..3 {
                press(ui, "+1", true)?;
            }
            press(ui, "Reset", false)?;
            Some(root.common())
        },
    },
    Demo {
        name: "focus",
        about: "focus and keys: Tab, Space, Right, Right, Space, Down",
        make: |ui| {
            let root = Focus::new(ui, None)?;
            for name in ["Tab", "space", "Right", "Right", "space", "Down"] {
                key(ui, name);
            }
            Some(root.common())
        },
    },
    Demo {
        name: "form",
        about: "RowLayout, ColumnLayout, GridLayout, Flow, Grid and Repeater",
        make: |ui| Form::new(ui, None).map(|root| root.common()),
    },
    Demo {
        name: "journal",
        about: "a ListView of 100,000 rows, scrolled to row 50,000",
        make: |ui| {
            let journal = Journal::new(ui, None)?;
            journal
                .entries
                .set(ui, tole::ModelRc::new(JournalRows::default()));
            let list = find_kind(ui, |kind| matches!(kind, tole::Kind::ListView(_)))?;
            ui.position_view_at_index(list, 50_000);
            Some(journal.common())
        },
    },
    Demo {
        name: "transitions",
        about: "states, a Transition, Behaviors and a looping animation, a second after the switch",
        make: |ui| {
            let root = Transitions::new(ui, None)?;
            ui.tick(0.0);
            click_at(ui, 430.0, 39.0);
            ui.update();
            ui.tick(1000.0);
            Some(root.common())
        },
    },
    Demo {
        name: "controls-light",
        about: "Fx.Controls in the light palette, a word double-clicked in a field",
        make: |ui| controls(ui, false),
    },
    Demo {
        name: "controls-dark",
        about: "the same, Theme.dark set",
        make: |ui| controls(ui, true),
    },
    Demo {
        name: "popups",
        about: "a ComboBox's list and a Menu open over a SplitView with a scrolled list",
        make: popups,
    },
    Demo {
        name: "file-chooser-dark",
        about: "the FileDialog asking for a private key, dark: a key chosen, its contents hidden",
        make: |ui| chooser(ui, true),
    },
    Demo {
        name: "file-chooser-light",
        about: "the same, light: all files, as a grid, a picture chosen and refused",
        make: |ui| chooser(ui, false),
    },
    Demo {
        name: "charts",
        about: "Canvas painters in Rust (histogram, sparkline, area) and Shapes in markup",
        make: charts,
    },
];

/// The charts demo: its three painters, Rust over made-up data, in the
/// theme's colours.
fn charts(ui: &mut Ui) -> Option<tole::Common> {
    use tole::{CanvasPainter, Path, Rect};
    let theme = fx_controls::Theme::instance(ui);
    theme.dark.set(ui, false);
    let (accent, fill, line, ink, red) = (
        theme.accent.get(ui),
        theme.accent_bg.get(ui),
        theme.line.get(ui),
        theme.ink4.get(ui),
        theme.red.get(ui),
    );
    let font = theme.sans(ui, 10.0, 400);
    let root = Charts::new(ui, None)?;
    // A wave with a bump: the same every run.
    let series = |n: usize, seed: f32| -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / n as f32;
                let wave = 0.55 + 0.25 * (t * 9.0 + seed).sin() + 0.12 * (t * 23.0 + seed).cos();
                let bump = (-((t - 0.7) * 9.0).powi(2)).exp() * 0.35;
                (wave + bump).clamp(0.05, 1.0)
            })
            .collect()
    };
    let bars = series(30, 0.3);
    root.histogram.set(
        ui,
        CanvasPainter::new(move |p| {
            let (w, h) = p.size();
            let base = h - 16.0;
            for k in 0..4 {
                let y = base - base * k as f32 / 3.0;
                p.line((0.0, y), (w, y), 1.0, line);
            }
            let step = w / bars.len() as f32;
            for (i, v) in bars.iter().enumerate() {
                let bar = (base - 4.0) * v;
                let color = if *v > 0.95 { red } else { accent };
                p.fill_rect(
                    Rect::new(i as f32 * step + 1.0, base - bar, step - 2.0, bar),
                    1.5,
                    color,
                );
            }
            for (i, label) in ["−30", "−20", "−10", "now"].iter().enumerate() {
                let x = (w - 18.0) * i as f32 / 3.0;
                p.text(label, &font, (x, h - 2.0), ink);
            }
        }),
    );
    let spark = series(40, 2.0);
    root.sparkline.set(
        ui,
        CanvasPainter::new(move |p| {
            let (w, h) = p.size();
            let mut path = Path::new();
            for (i, v) in spark.iter().enumerate() {
                let x = w * i as f32 / (spark.len() - 1) as f32;
                let y = h - 2.0 - (h - 4.0) * v;
                if i == 0 {
                    let _ = path.move_to(x, y);
                } else {
                    let _ = path.line_to(x, y);
                }
            }
            p.path(&path, Color::TRANSPARENT, red, 1.5);
        }),
    );
    let memory = series(60, 4.0);
    root.area.set(
        ui,
        CanvasPainter::new(move |p| {
            let (w, h) = p.size();
            let point =
                |i: usize, v: f32| (w * i as f32 / (memory.len() - 1) as f32, h - (h - 6.0) * v);
            let mut top = Path::new();
            let mut under = Path::new();
            let _ = under.move_to(0.0, h);
            for (i, v) in memory.iter().enumerate() {
                let (x, y) = point(i, *v);
                if i == 0 {
                    let _ = top.move_to(x, y);
                } else {
                    let _ = top.line_to(x, y);
                }
                let _ = under.line_to(x, y);
            }
            let _ = under.line_to(w, h).close();
            p.path(&under, fill, Color::TRANSPARENT, 0.0);
            p.path(&top, Color::TRANSPARENT, accent, 1.5);
        }),
    );
    Some(root.common())
}

/// The popups demo: the combo box opened by a click, the menu by the
/// program, an item of it under the pointer, and the list scrolled.
fn popups(ui: &mut Ui) -> Option<tole::Common> {
    let theme = fx_controls::Theme::instance(ui);
    theme.dark.set(ui, false);
    let root = Popups::new(ui, None)?;
    let list = find_kind(ui, |k| matches!(k, tole::Kind::ListView(_)))?;
    if let Some(flick) = ui.flickable(list) {
        flick.content_y.set(ui, 64.0);
    }
    press(ui, "pixel7", true)?;
    let menu = find_text(ui, "Copy record").and_then(|t| popup_of(ui, t))?;
    ui.open_popup(menu);
    press(ui, "Copy cursor", false)?;
    Some(root.common())
}

/// The clock the file chooser's pictures are drawn at: 2026-10-24,
/// noon UTC, so its dates do not move with the day it is drawn.
pub const FIXTURE_NOW: i64 = 1_792_843_200;

/// A home folder for the file chooser's demos and tests, made once per
/// process under the temporary folder: keys (the private ones only
/// look like keys: written here, never kept in the tree), pictures drawn
/// here, documents, each with a set age.
///
/// A file that cannot be written is left out.
#[must_use]
pub fn fixture_home() -> PathBuf {
    static MADE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    MADE.get_or_init(|| {
        let home = std::env::temp_dir().join(format!("tole-examples-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let day = 86_400;
        let private = |kind: &str, lines: usize| {
            let mut text = format!("-----BEGIN {kind} PRIVATE KEY-----\n");
            for _ in 0..lines {
                text.push_str("not-a-real-key-not-a-real-key-not-a-real-key-not-a-real-key-not-\n");
            }
            text.push_str(&format!("-----END {kind} PRIVATE KEY-----\n"));
            text.into_bytes()
        };
        let public = |who: &str| {
            format!("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIK7v3pQm9yX2cF1nB8dLhT0sW4eRjZ6aUo5gNiVkY2Mx {who}\n")
                .into_bytes()
        };
        let files: Vec<(&str, Vec<u8>, i64)> = vec![
            ("machines/keys/id_ed25519", private("OPENSSH", 5), 3600),
            ("machines/keys/id_ed25519.pub", public("deploy@workstation"), 3600),
            ("machines/keys/staging_rsa", private("OPENSSH", 40), 42 * day),
            ("machines/keys/staging_rsa.pub", public("staging@workstation"), 42 * day),
            ("machines/keys/archive/old_prod.pem", private("RSA", 25), 235 * day),
            ("machines/keys/.known_hosts", b"web-01 ssh-ed25519 AAAA\n".to_vec(), 3600),
            (
                "machines/configs/web-01.toml",
                b"[host]\nname = \"web-01\"\naddress = \"10.0.4.21\"\nuser = \"deploy\"\n".to_vec(),
                22 * day,
            ),
            ("machines/inventory.yaml", b"machines:\n  - web-01\n  - staging\n".to_vec(), 4 * day),
            ("machines/README.md", b"# machines\n".to_vec(), 9 * day),
            (".ssh/id_rsa", private("OPENSSH", 40), 289 * day),
            (".ssh/id_rsa.pub", public("me@workstation"), 289 * day),
            (
                ".ssh/config",
                b"Host web-01\n  HostName 10.0.4.21\n  User deploy\n  IdentityFile ~/machines/keys/id_ed25519\n".to_vec(),
                6 * day,
            ),
            (".ssh/known_hosts", b"web-01 ssh-ed25519 AAAA\n".repeat(40), 3600),
            ("Pictures/harbour-dusk.png", picture(320, 200, [233, 120, 92], [44, 62, 120]), 3600),
            ("Pictures/desk-plant.png", picture(240, 240, [126, 166, 245], [47, 163, 90]), day),
            ("Documents/notes.md", b"# Notes\n\nThe staging key rotates on the first.\n".to_vec(), 10 * day),
            ("Downloads/deploy-key.pem", private("RSA", 25), day),
            ("Downloads/invoice.pdf", b"%PDF-1.4\n%not a real invoice\n".repeat(200), 15 * day),
        ];
        for (name, bytes, age) in &files {
            let path = home.join(name);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&path, bytes);
            set_age(&path, *age);
        }
        for dir in ["machines/keys/archive", "machines/keys", "machines/configs", "machines", ".ssh", "Pictures", "Documents", "Downloads"] {
            set_age(&home.join(dir), 2 * day);
        }
        home
    })
    .clone()
}

/// Make `path` changed `age` seconds before [`FIXTURE_NOW`].
fn set_age(path: &std::path::Path, age: i64) {
    let when = std::time::UNIX_EPOCH
        + std::time::Duration::from_secs(u64::try_from(FIXTURE_NOW - age).unwrap_or(0));
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.set_modified(when);
    }
}

/// A picture `w` by `h`: a gradient from `a` at the top left to `b` at
/// the bottom right, with a light disc, as a PNG file's bytes.
fn picture(w: u32, h: u32, a: [u8; 3], b: [u8; 3]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let t = (x + y) as f32 / (w + h) as f32;
            let (dx, dy) = (x as f32 - w as f32 * 0.7, y as f32 - h as f32 * 0.35);
            let sun = (dx * dx + dy * dy).sqrt() < h as f32 * 0.16;
            for (&from, &to) in a.iter().zip(b.iter()) {
                let (from, to) = (f32::from(from), f32::from(to));
                let v = if sun { 250.0 } else { from + (to - from) * t };
                rgba.push(v as u8);
            }
            rgba.push(255);
        }
    }
    tole::png::encode(w, h, &rgba)
}

/// The file chooser demo, as the lode viewer asks for a key. Dark: open
/// on `~/machines/keys`, a key chosen, its preview hiding what is in it.
/// Light: `~/Pictures` as a grid under "All files", a picture chosen,
/// which the line at the foot says is not a key.
fn chooser(ui: &mut Ui, dark: bool) -> Option<tole::Common> {
    tole::fix_home_and_clock(Some(fixture_home()), Some(FIXTURE_NOW));
    let theme = fx_controls::Theme::instance(ui);
    theme.dark.set(ui, dark);
    let root = Chooser::new(ui, None)?;
    let model = find_kind(ui, |k| matches!(k, tole::Kind::FolderListModel(_)))?;
    let props = ui.folder_list(model)?;
    let dialog = popup_of(ui, model)?;
    ui.open_popup(dialog);
    if dark {
        props.folder.set(ui, "~/machines/keys".into());
        ui.update();
        press(ui, "id_ed25519", true)?;
    } else {
        props.folder.set(ui, "~/Pictures".into());
        ui.update();
        press(ui, "All files", true)?;
        // The grid button, beside the list's.
        let at = ui.rect_of(dialog)?;
        click_at(ui, at.x + 72.0, at.y + 77.0);
        ui.update();
        press(ui, "harbour-dusk.png", true)?;
        // Over the place it would go to.
        press(ui, ".ssh", false)?;
        return Some(root.common());
    }
    ui.pointer_leave();
    Some(root.common())
}

/// The `Popup` around `item`.
pub fn popup_of(ui: &Ui, item: tole::ItemId) -> Option<tole::ItemId> {
    let mut at = ui.parent(item);
    while let Some(above) = at {
        if ui.popup(above).is_some() {
            return Some(above);
        }
        at = ui.parent(above);
    }
    None
}

/// The controls demo, light or dark, with "nazuna" double-clicked in its
/// first field.
fn controls(ui: &mut Ui, dark: bool) -> Option<tole::Common> {
    let theme = fx_controls::Theme::instance(ui);
    theme.dark.set(ui, dark);
    let root = Controls::new(ui, None)?;
    let field = find_input(ui, "nazuna.local")?;
    let rect = ui.rect_of(field)?;
    for _ in 0..2 {
        ui.pointer_move(rect.x + 12.0, rect.y + rect.h / 2.0);
        ui.pointer_press();
        ui.pointer_release();
    }
    ui.pointer_leave();
    Some(root.common())
}

/// The first `TextInput` or `TextEdit`, anywhere, holding `text`.
pub fn find_input(ui: &mut Ui, text: &str) -> Option<tole::ItemId> {
    let mut stack: Vec<tole::ItemId> = ui.roots().to_vec();
    while let Some(item) = stack.pop() {
        if let Some(input) = ui.text_input(item)
            && &*ui.get(input.text) == text
        {
            return Some(item);
        }
        stack.extend(ui.children(item).iter().rev());
    }
    None
}

/// Press and release the pointer at (`x`, `y`).
pub fn click_at(ui: &mut Ui, x: f32, y: f32) {
    ui.pointer_move(x, y);
    ui.pointer_press();
    ui.pointer_release();
    ui.pointer_leave();
}

/// The transitions demo as an animated PNG at scale 1: the switch turned
/// on, then off, 25 frames a second.
#[must_use]
pub fn transitions_animated() -> Option<Vec<u8>> {
    let mut ui = Ui::new(fonts());
    let root = Transitions::new(&mut ui, None)?;
    let (w, h) = (
        ui.get(root.common().width).round() as u32,
        ui.get(root.common().height).round() as u32,
    );
    let mut frames = Vec::new();
    for frame in 0..60_u32 {
        let t = f64::from(frame) * 40.0;
        if frame == 5 || frame == 35 {
            click_at(&mut ui, 430.0, 39.0);
        }
        ui.update();
        ui.tick(t);
        frames.push(ui.render((w, h), Color::hex(0xfa_fa_f8))?.data().to_vec());
    }
    Some(tole::png::encode_animated(w, h, &frames, 40))
}

/// The journal demo's rows: 100,000 of them, each made when asked for
/// and never kept.
#[derive(Debug, Default)]
pub struct JournalRows {
    /// Says nothing ever changes.
    tracker: tole::ModelTracker,
}

/// A row of the journal: a heading every 40 rows, records between.
const JOURNAL_ROWS: usize = 100_000;

impl tole::Model for JournalRows {
    type Row = Entry;

    fn row_count(&self) -> usize {
        JOURNAL_ROWS
    }

    fn row(&self, index: usize) -> Option<Entry> {
        if index >= JOURNAL_ROWS {
            return None;
        }
        let seconds = 8 * 3600 + index * 3;
        let time = format!(
            "{:02}:{:02}:{:02}",
            seconds / 3600 % 24,
            seconds / 60 % 60,
            seconds % 60
        );
        if index.is_multiple_of(40) {
            return Some(Entry {
                kind: EntryKind::Header,
                text: format!("Boot {} · {time}", index / 40 + 1).into(),
                ..Entry::default()
            });
        }
        const SOURCES: [&str; 6] = ["kernel", "hyprix", "netd", "lode", "authd", "sshd"];
        const TEXTS: [&str; 6] = [
            "link up at 1 Gbit/s, full duplex",
            "output HDMI-A-1: 2560x1440 at 120 Hz",
            "lease renewed for 192.168.9.193",
            "forwarding 1,204 records over TLS",
            "session opened for sebastian",
            "accepted publickey from 192.168.9.20 port 51022",
        ];
        let hash = index.wrapping_mul(2_654_435_761) >> 7;
        let level = match hash % 23 {
            0 => 3,
            1 | 2 => 4,
            _ => 6,
        };
        Some(Entry {
            kind: EntryKind::Record,
            level,
            time: time.into(),
            source: SOURCES
                .get(hash % SOURCES.len())
                .copied()
                .unwrap_or_default()
                .into(),
            text: format!(
                "{} (#{})",
                TEXTS
                    .get((hash / 7) % TEXTS.len())
                    .copied()
                    .unwrap_or_default(),
                tole::group(index as i32)
            )
            .into(),
        })
    }

    fn row_height(&self, index: usize) -> Option<f32> {
        Some(if index.is_multiple_of(40) { 34.0 } else { 24.0 })
    }

    fn tracker(&self) -> &tole::ModelTracker {
        &self.tracker
    }
}

/// The first item, anywhere, whose kind `matches` says.
pub fn find_kind(ui: &Ui, matches: impl Fn(&tole::Kind) -> bool) -> Option<tole::ItemId> {
    let mut stack: Vec<tole::ItemId> = ui.roots().to_vec();
    while let Some(item) = stack.pop() {
        if ui.kind(item).is_some_and(|kind| matches(&kind)) {
            return Some(item);
        }
        stack.extend(ui.children(item).iter().rev());
    }
    None
}

/// The fonts the demos draw with: `TOLE_FONTS` when set, else the Ferrix
/// tree's Inter beside this checkout, so a screenshot does not depend on
/// what the machine has installed; where there is no tree (a Ferrix image
/// running a demo), the system's, `/usr/share/fonts`.
#[must_use]
pub fn fonts() -> Fonts {
    if let Some(dir) = std::env::var_os("TOLE_FONTS") {
        let mut fonts = Fonts::new();
        let _ = fonts.add_dir(std::path::Path::new(&dir));
        return fonts;
    }
    let tree =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../../../assets/fonts/inter");
    if tree.is_dir() {
        let mut fonts = Fonts::new();
        let _ = fonts.add_dir(&tree);
        return fonts;
    }
    Fonts::system()
}

/// `demo` in a window on the Wayland desktop (`$WAYLAND_DISPLAY`), until
/// it is closed.
///
/// # Errors
///
/// No compositor, or the connection failed.
pub fn run_window(demo: &Demo) -> Result<(), String> {
    let mut ui = Ui::new(fonts());
    let root = (demo.make)(&mut ui).ok_or("the demo was not made")?;
    let size = (
        ui.get(root.width).round() as u32,
        ui.get(root.height).round() as u32,
    );
    let options = tole::window::WindowOptions {
        title: format!("tole: {}", demo.name),
        app_id: "rocks.magical.tole-examples".to_owned(),
        size,
        background: Color::hex(0xff_ff_ff),
        ..tole::window::WindowOptions::default()
    };
    let mut window =
        tole::window::Window::open(root, options).map_err(|error| error.to_string())?;
    let mut up = false;
    let mut said = String::new();
    while !window.is_closed() {
        let _ = window
            .dispatch(&mut ui, None)
            .map_err(|error| error.to_string())?;
        // Where the keys are and how many popups are open, each time it
        // changes: a boot drives the window by keys and reads this back.
        if up {
            let now = keys_line(&mut ui);
            if now != said {
                use std::io::Write as _;
                let mut out = std::io::stdout();
                let _ = writeln!(out, "tole-examples: {now}");
                let _ = out.flush();
                said = now;
            }
        }
        // Said once, for a boot that waits for the window.
        if !up && let Some((w, h)) = window.size() {
            up = true;
            use std::io::Write as _;
            let mut out = std::io::stdout();
            let _ = writeln!(out, "tole-examples: {} is up, {w}x{h}", demo.name);
            let _ = out.flush();
        }
    }
    Ok(())
}

/// Where the keys are -- a text field and its text, a popup, or
/// another item -- and how many popups are open.
#[must_use]
pub fn keys_line(ui: &mut Ui) -> String {
    let keys = match ui.active_focus_item() {
        None => "nowhere".to_owned(),
        Some(item) => match (ui.text_input(item), ui.popup(item)) {
            (Some(input), _) => format!("a text field holding \"{}\"", ui.get(input.text)),
            (None, Some(_)) => "a popup".to_owned(),
            (None, None) => "an item".to_owned(),
        },
    };
    let open = ui.open_popups().len();
    format!("keys on {keys}; {open} popups open")
}

/// `demo` drawn at `scale`, as a PNG file's bytes.
#[must_use]
pub fn render(demo: &Demo, scale: f32) -> Option<Vec<u8>> {
    let mut ui = Ui::new(fonts());
    ui.set_scale(scale);
    let root = (demo.make)(&mut ui)?;
    let width = ui.get(root.width).round().max(1.0) as u32;
    let height = ui.get(root.height).round().max(1.0) as u32;
    let frame = ui.render((width, height), Color::TRANSPARENT)?;
    Some(tole::png::encode(
        frame.width(),
        frame.height(),
        frame.data(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{DEMOS, keys_line, render};

    /// What the `tole-demo` boot does by QEMU's keys, done headless: the
    /// combo box's list opened by Space takes the keys, and Escape gives
    /// them back to the box.
    #[test]
    fn the_combo_box_list_takes_the_keys_and_gives_them_back() {
        let demo = DEMOS
            .iter()
            .find(|d| d.name == "controls-light")
            .expect("the demo");
        let mut ui = tole::Ui::new(super::fonts());
        let root = (demo.make)(&mut ui).expect("made");
        root.width.set(&mut ui, 700.0);
        root.height.set(&mut ui, 520.0);
        ui.set_window_active(true);
        ui.update();
        let key = |ui: &mut tole::Ui, name: &str| {
            let _ = ui.key_press(&tole::KeyEvent::named(name));
            ui.update();
        };
        assert!(keys_line(&mut ui).starts_with("keys on a text field"));
        for _ in 0..4 {
            key(&mut ui, "Tab");
        }
        assert_eq!(keys_line(&mut ui), "keys on an item; 0 popups open");
        key(&mut ui, "space");
        assert_eq!(keys_line(&mut ui), "keys on a popup; 1 popups open");
        key(&mut ui, "Escape");
        assert_eq!(keys_line(&mut ui), "keys on an item; 0 popups open");
    }

    #[test]
    fn the_screenshots_are_what_the_demos_draw() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/screenshots");
        for demo in &DEMOS {
            let drawn = render(demo, 2.0).expect("drawn");
            let kept = std::fs::read(dir.join(format!("{}.png", demo.name))).unwrap_or_default();
            assert!(
                drawn == kept,
                "docs/screenshots/{}.png is not what the demo draws: run `cargo run -p tole-examples -- --all docs/screenshots`",
                demo.name
            );
        }
    }

    #[test]
    fn the_counter_counts_and_says_so() {
        use super::{Counter, press};
        let mut ui = tole::Ui::new(super::fonts());
        let counter = Counter::new(&mut ui, None).expect("made");
        press(&mut ui, "+1", true).expect("pressed");
        press(&mut ui, "+1", true).expect("pressed");
        assert_eq!(counter.count.get(&mut ui), 2);
        assert!(super::find_text(&mut ui, "Clicked 2 times").is_some());
        press(&mut ui, "−1", true).expect("pressed");
        press(&mut ui, "−1", true).expect("pressed");
        press(&mut ui, "−1", true).expect("pressed");
        assert_eq!(counter.count.get(&mut ui), 0, "the `if` keeps it at nought");
    }

    #[test]
    fn the_program_hears_a_signal_through_the_generated_enum() {
        use super::{Button, ButtonSignal, press};
        use tole::Element;
        let mut ui = tole::Ui::new(super::fonts());
        let button = Button::new(&mut ui, None).expect("made");
        button.label.set(&mut ui, "Go".into());
        press(&mut ui, "Go", true).expect("pressed");
        let heard: Vec<ButtonSignal> = ui
            .take_signals()
            .iter()
            .filter_map(|e| button.signal(e))
            .collect();
        assert_eq!(heard, [ButtonSignal::Clicked]);
        // Its width follows its label.
        let narrow = ui.get(button.common().width);
        button.label.set(&mut ui, "A longer label".into());
        assert!(ui.get(button.common().width) > narrow + 40.0);
    }

    #[test]
    fn tab_arrows_and_space_work_the_tiles() {
        use super::{Focus, key};
        let mut ui = tole::Ui::new(super::fonts());
        let _focus = Focus::new(&mut ui, None).expect("made");
        let label_of_focus = |ui: &mut tole::Ui| {
            let item = ui.active_focus_item().expect("something has the focus");
            let label = ui.children(item)[0];
            let text = ui.text(label).expect("a tile's first child is its label");
            ui.get(text.text).to_string()
        };
        key(&mut ui, "Tab");
        assert_eq!(label_of_focus(&mut ui), "Wi-Fi");
        key(&mut ui, "Tab");
        key(&mut ui, "Tab");
        key(&mut ui, "Tab");
        assert_eq!(label_of_focus(&mut ui), "Dark", "tree order");
        key(&mut ui, "Up");
        assert_eq!(label_of_focus(&mut ui), "Wi-Fi", "KeyNavigation.up");
        key(&mut ui, "ISO_Left_Tab");
        assert_eq!(label_of_focus(&mut ui), "Quiet", "Shift+Tab wraps");
        // A click gives the focus and toggles.
        super::press(&mut ui, "Bluetooth", true).expect("clicked");
        assert_eq!(label_of_focus(&mut ui), "Bluetooth");
        let on = |ui: &mut tole::Ui| {
            let item = ui.active_focus_item().expect("focus");
            let rect = ui.rectangle(item).expect("a tile is a rectangle");
            ui.get(rect.color) == tole::Color::hex(0x2a62c9)
        };
        assert!(on(&mut ui));
        key(&mut ui, "space");
        assert!(!on(&mut ui));
    }

    #[test]
    fn the_journal_makes_a_screenful_of_its_hundred_thousand_rows() {
        let mut ui = tole::Ui::new(super::fonts());
        let root = (DEMOS[5].make)(&mut ui).expect("made");
        let _ = ui.render((600, 420), tole::Color::TRANSPARENT);
        let list = super::find_kind(&ui, |k| matches!(k, tole::Kind::ListView(_))).expect("a list");
        let (held, made) = ui.view_delegates(list).expect("a view");
        assert!(held < 30 && made < 30, "{held} held, {made} made");
        assert!(super::find_text(&mut ui, "100,000 entries").is_some());
        // Scrolled through a thousand rows by the wheel, it makes no more
        // than it holds.
        ui.pointer_move(300.0, 200.0);
        for _ in 0..1000 {
            ui.pointer_wheel(8.0);
        }
        let (_, after) = ui.view_delegates(list).expect("a view");
        assert!(after < 40, "{after} made");
        let _ = root;
    }

    #[test]
    fn the_animation_is_what_the_demo_plays() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../docs/screenshots/transitions-animated.png");
        let kept = std::fs::read(path).unwrap_or_default();
        assert!(
            super::transitions_animated().expect("played") == kept,
            "docs/screenshots/transitions-animated.png is stale: run `cargo run -p tole-examples -- --all docs/screenshots`"
        );
    }

    #[test]
    fn the_switch_slides_and_the_state_follows() {
        let mut ui = tole::Ui::new(super::fonts());
        let root = super::Transitions::new(&mut ui, None).expect("made");
        let panel = super::find_text(&mut ui, "The panel")
            .and_then(|t| ui.parent(t))
            .expect("panel");
        let x = |ui: &mut tole::Ui| ui.get(ui.common(panel).expect("lives").x);
        assert_eq!(x(&mut ui), 440.0);
        ui.tick(0.0);
        super::click_at(&mut ui, 430.0, 39.0);
        ui.update();
        ui.tick(250.0);
        let mid = x(&mut ui);
        assert!(mid < 300.0 && mid > 200.0, "on its way: {mid}");
        ui.tick(600.0);
        assert_eq!(x(&mut ui), 220.0);
        assert!(ui.animating(), "the dot pulses for ever");
        let _ = root;
    }

    #[test]
    fn radio_buttons_beside_each_other_are_exclusive() {
        use super::fx_controls::RadioButton;
        use tole::Element;
        let mut ui = tole::Ui::new(super::fonts());
        let column = tole::handles::Column::new(&mut ui, None).expect("made");
        let mut radios = Vec::new();
        for label in ["Light", "Dark", "System"] {
            let radio = RadioButton::new(&mut ui, Some(column.id())).expect("made");
            radio.text.set(&mut ui, label.into());
            radios.push(radio);
        }
        let other = RadioButton::new(&mut ui, None).expect("a radio button elsewhere");
        other.checked.set(&mut ui, true);
        radios[1].checked.set(&mut ui, true);
        ui.update();
        super::press(&mut ui, "System", true).expect("clicked");
        ui.update();
        let checked: Vec<bool> = radios.iter().map(|r| r.checked.get(&mut ui)).collect();
        assert_eq!(checked, [false, false, true]);
        assert!(
            other.checked.get(&mut ui),
            "another parent's is not touched"
        );
        // Space on the focused one checks it, and the others follow.
        ui.force_active_focus(radios[0].id());
        super::key(&mut ui, "space");
        ui.update();
        let checked: Vec<bool> = radios.iter().map(|r| r.checked.get(&mut ui)).collect();
        assert_eq!(checked, [true, false, false]);
    }

    #[test]
    fn check_boxes_switches_buttons_and_tabs_answer() {
        use super::fx_controls::{Button, ButtonSignal, CheckBox, Switch, TabBar, Theme};
        use tole::Element;
        let mut ui = tole::Ui::new(super::fonts());
        let column = tole::handles::Column::new(&mut ui, None).expect("made");
        let check = CheckBox::new(&mut ui, Some(column.id())).expect("made");
        check.text.set(&mut ui, "Wrap".into());
        let switch = Switch::new(&mut ui, Some(column.id())).expect("made");
        switch.text.set(&mut ui, "Live".into());
        let button = Button::new(&mut ui, Some(column.id())).expect("made");
        button.text.set(&mut ui, "Go".into());
        let tabs = TabBar::new(&mut ui, Some(column.id())).expect("made");
        tabs.tabs
            .set(&mut ui, vec!["One".into(), "Two".into(), "Three".into()]);
        super::press(&mut ui, "Wrap", true).expect("clicked");
        assert!(check.checked.get(&mut ui));
        super::press(&mut ui, "Live", true).expect("clicked");
        assert!(switch.checked.get(&mut ui));
        let _ = ui.take_signals();
        super::press(&mut ui, "Go", true).expect("clicked");
        ui.force_active_focus(button.id());
        super::key(&mut ui, "Return");
        let heard = ui
            .take_signals()
            .iter()
            .filter(|e| button.signal(e) == Some(ButtonSignal::Clicked))
            .count();
        assert_eq!(heard, 2, "a click and Return");
        super::press(&mut ui, "Three", true).expect("clicked");
        assert_eq!(tabs.current_index.get(&mut ui), 2);
        ui.force_active_focus(tabs.id());
        super::key(&mut ui, "Left");
        super::key(&mut ui, "Left");
        super::key(&mut ui, "Left");
        assert_eq!(tabs.current_index.get(&mut ui), 0, "and not past the first");
        // The theme changes every control's colours at once.
        let light = ui.get(button.base.props.color);
        let theme = Theme::instance(&mut ui);
        theme.dark.set(&mut ui, true);
        let dark = ui.get(button.base.props.color);
        assert_ne!(light, dark);
        assert_eq!(dark, theme.bg.get(&mut ui));
        assert_eq!(Theme::instance(&mut ui), theme, "one per Ui");
    }

    #[test]
    fn a_text_field_is_typed_into_and_its_alias_follows() {
        use super::fx_controls::{TextField, TextFieldSignal};
        use tole::Element;
        let mut ui = tole::Ui::new(super::fonts());
        let field = TextField::new(&mut ui, None).expect("made");
        field.placeholder_text.set(&mut ui, "Host".into());
        let _ = ui.render((200, 30), tole::Color::TRANSPARENT);
        // A click beside the text focuses it too.
        let rect = ui.rect_of(field.id()).expect("placed");
        ui.pointer_move(rect.x + 4.0, rect.y + 4.0);
        ui.pointer_press();
        ui.pointer_release();
        for key in ["n", "a", "z", "u", "n", "a"] {
            super::key(&mut ui, key);
        }
        assert_eq!(
            &*field.text.get(&mut ui),
            "nazuna",
            "typed through the alias"
        );
        assert!(super::find_text(&mut ui, "Host").is_some());
        let shown = |ui: &mut tole::Ui| {
            let text = super::find_text(ui, "Host").expect("placeholder");
            ui.get(ui.common(text).expect("lives").visible)
        };
        assert!(!shown(&mut ui), "the placeholder hides once typed into");
        field.text.set(&mut ui, "".into());
        assert!(shown(&mut ui));
        super::key(&mut ui, "Return");
        assert!(
            ui.take_signals()
                .iter()
                .any(|e| field.signal(e) == Some(TextFieldSignal::Accepted))
        );
    }

    #[test]
    fn a_combo_box_opens_over_everything_and_a_press_outside_closes_it() {
        let mut ui = tole::Ui::new(super::fonts());
        let root = super::Popups::new(&mut ui, None).expect("made");
        let _ = ui.render((720, 420), tole::Color::TRANSPARENT);
        let shown = |ui: &mut tole::Ui, label: &str| {
            let text = super::find_text(ui, label).expect("there");
            let popup = super::popup_of(ui, text).expect("in a popup");
            ui.get(ui.common(popup).expect("lives").visible)
        };
        assert!(!shown(&mut ui, "n150"));
        super::press(&mut ui, "pixel7", true).expect("clicked");
        assert!(shown(&mut ui, "n150"));
        // Chosen: the list closes and the combo box says it.
        super::press(&mut ui, "dk1", true).expect("clicked");
        assert!(!shown(&mut ui, "n150"));
        assert!(super::find_text(&mut ui, "dk1").is_some());
        // A press outside closes it and does nothing else: "Actions"
        // under it is not clicked.
        super::press(&mut ui, "dk1", true).expect("clicked");
        assert!(shown(&mut ui, "n150"));
        super::press(&mut ui, "Actions", true).expect("clicked");
        assert!(!shown(&mut ui, "n150"));
        assert!(!shown(&mut ui, "Copy record"), "the press was the popup's");
        // Now Actions opens the menu; a click on an item closes it; Escape
        // closes it too.
        super::press(&mut ui, "Actions", true).expect("clicked");
        assert!(shown(&mut ui, "Copy record"));
        super::press(&mut ui, "Copy cursor", true).expect("clicked");
        assert!(!shown(&mut ui, "Copy record"));
        super::press(&mut ui, "Actions", true).expect("clicked");
        super::key(&mut ui, "Escape");
        assert!(!shown(&mut ui, "Copy record"));
        // Keys step the combo box.
        let combo = super::find_text(&mut ui, "dk1")
            .and_then(|t| ui.parent(t))
            .expect("the combo box");
        ui.force_active_focus(combo);
        super::key(&mut ui, "Down");
        let label = ui.children(combo)[0];
        let text = ui.text(label).expect("its label");
        assert_eq!(&*ui.get(text.text), "n150");
        let _ = root;
    }

    #[test]
    fn a_split_view_handle_drags_and_a_scroll_bar_scrolls() {
        let mut ui = tole::Ui::new(super::fonts());
        let _root = super::Popups::new(&mut ui, None).expect("made");
        let _ = ui.render((720, 420), tole::Color::TRANSPARENT);
        let split =
            super::find_kind(&ui, |k| matches!(k, tole::Kind::SplitView(_))).expect("split");
        let left = ui.children(split)[0];
        let width = |ui: &mut tole::Ui| ui.get(ui.common(left).expect("lives").width);
        assert_eq!(width(&mut ui), 220.0);
        ui.pointer_move(221.0, 300.0);
        assert_eq!(ui.take_cursor(), Some(tole::Cursor::ColResize));
        ui.pointer_press();
        ui.pointer_move(301.0, 300.0);
        ui.pointer_release();
        assert_eq!(width(&mut ui), 300.0);
        ui.pointer_move(1.0, 300.0);
        ui.pointer_press();
        ui.pointer_move(-500.0, 300.0);
        ui.pointer_release();
        // Not past the handle's own room: the press at 1 is no handle.
        assert_eq!(width(&mut ui), 300.0);
        // The bar: a drag to its foot scrolls the list to its end.
        let list = super::find_kind(&ui, |k| matches!(k, tole::Kind::ListView(_))).expect("list");
        let flick = ui.flickable(list).expect("flick");
        let bar_x = 300.0 - 6.0;
        ui.pointer_move(bar_x, 30.0);
        ui.pointer_press();
        ui.pointer_move(bar_x, 1000.0);
        ui.pointer_release();
        let end = ui.get(flick.content_y);
        let most = 40.0 * 28.0 - (420.0 - 16.0);
        assert!((end - most).abs() < 1.0, "{end} of {most}");
    }

    /// Whether a `Text` saying `label` is shown: it and every item above
    /// it visible.
    fn shown(ui: &mut tole::Ui, label: &str) -> bool {
        let mut stack: Vec<tole::ItemId> = ui.roots().to_vec();
        while let Some(item) = stack.pop() {
            let Some(common) = ui.common(item) else {
                continue;
            };
            if !ui.get(common.visible) {
                continue;
            }
            if let Some(text) = ui.text(item)
                && &*ui.get(text.text) == label
            {
                return true;
            }
            stack.extend(ui.children(item).iter().rev());
        }
        false
    }

    fn type_text(ui: &mut tole::Ui, text: &str) {
        for c in text.chars() {
            super::key(ui, &c.to_string());
        }
    }

    #[test]
    fn the_file_dialog_walks_filters_searches_and_chooses_by_key() {
        use super::fx_controls::FileDialogSignal;
        tole::fix_home_and_clock(Some(super::fixture_home()), Some(super::FIXTURE_NOW));
        let mut ui = tole::Ui::new(super::fonts());
        let root = tole::handles::Item::new(&mut ui, None).expect("made");
        root.common.width.set(&mut ui, 1000.0);
        root.common.height.set(&mut ui, 700.0);
        let handle = super::fx_controls::FileDialog::new(&mut ui, Some(root.id)).expect("made");
        handle.accepted_types.set(&mut ui, "[private]".into());
        handle.accepted_label.set(&mut ui, "Private keys".into());
        handle.accepted_noun.set(&mut ui, "private keys".into());
        handle.open_label.set(&mut ui, "Use key".into());
        handle.folder.set(&mut ui, "~/.ssh".into());
        let dialog = tole::Element::id(&handle);
        let model =
            super::find_kind(&ui, |k| matches!(k, tole::Kind::FolderListModel(_))).expect("model");
        let props = ui.folder_list(model).expect("props");
        let draw = |ui: &mut tole::Ui| {
            let _ = ui.render((1000, 700), tole::Color::TRANSPARENT);
        };
        ui.open_popup(dialog);
        draw(&mut ui);
        // It opens in ~/.ssh, private keys only.
        assert!(shown(&mut ui, "id_rsa"));
        assert!(!shown(&mut ui, "id_rsa.pub"));
        assert!(!shown(&mut ui, "config"));
        assert!(shown(&mut ui, "Pick one of the private keys"));
        // All files: the rest shows, and a public key is refused, saying why.
        super::press(&mut ui, "All files", true).expect("clicked");
        draw(&mut ui);
        assert!(shown(&mut ui, "config"));
        super::press(&mut ui, "id_rsa.pub", true).expect("clicked");
        draw(&mut ui);
        assert!(shown(&mut ui, "Public key: use the file without .pub"));
        let _ = ui.take_signals();
        super::press(&mut ui, "Use key", true).expect("clicked");
        assert!(ui.get(tole::Element::common(&root).visible));
        assert!(
            ui.get(ui.common(dialog).expect("common").visible),
            "still open"
        );
        // Backspace goes up, to the home folder.
        super::key(&mut ui, "BackSpace");
        draw(&mut ui);
        assert_eq!(&*ui.get(props.display_path), "~");
        assert!(shown(&mut ui, "machines"));
        // The search: the name contains it; then as a regular expression,
        // which cannot be read with a lone bracket.
        super::press(&mut ui, "Filter by name", true).expect("clicked");
        type_text(&mut ui, "own");
        draw(&mut ui);
        assert!(shown(&mut ui, "Downloads"));
        assert!(!shown(&mut ui, "machines"));
        super::press(&mut ui, "Aa", true).expect("clicked");
        draw(&mut ui);
        super::press(&mut ui, "Regex", true).expect("clicked");
        draw(&mut ui);
        assert_eq!(ui.get(props.filter_mode), tole::search_mode::REGEX);
        type_text(&mut ui, "(");
        draw(&mut ui);
        assert!(!ui.get(props.filter_error).is_empty());
        assert!(shown(&mut ui, "Any text, then .png"), "the help shows");
        for _ in 0..4 {
            super::key(&mut ui, "BackSpace");
        }
        draw(&mut ui);
        assert!(ui.get(props.filter_error).is_empty());
        // A pattern of one's own that cannot be read says so.
        super::press(&mut ui, "Custom", true).expect("clicked");
        draw(&mut ui);
        let field = super::find_input(&mut ui, "[image] | *.txt").expect("pattern field");
        let input = ui.text_input(field).expect("input");
        input.text.set(&mut ui, "[pictur".into());
        draw(&mut ui);
        assert!(ui.get(props.accept_error).contains("unmatched bracket"));
        input.text.set(&mut ui, "[image]".into());
        // The keys: into ~/machines/keys, Down twice, Return chooses.
        super::press(&mut ui, "Private keys", true).expect("clicked");
        props.folder.set(&mut ui, "~/machines/keys".into());
        draw(&mut ui);
        let _ = ui.take_signals();
        super::key(&mut ui, "Down");
        super::key(&mut ui, "Down");
        draw(&mut ui);
        assert!(shown(&mut ui, "Private key \u{b7} 395 B"));
        super::key(&mut ui, "Return");
        let chosen: Vec<FileDialogSignal> = ui
            .take_signals()
            .iter()
            .filter_map(|e| handle.signal(e))
            .filter(|s| {
                matches!(
                    s,
                    FileDialogSignal::Accepted(_) | FileDialogSignal::Rejected
                )
            })
            .collect();
        let want = super::fixture_home().join("machines/keys/id_ed25519");
        assert_eq!(
            chosen,
            [FileDialogSignal::Accepted(
                want.to_string_lossy().as_ref().into()
            )]
        );
        assert!(
            !ui.get(ui.common(dialog).expect("common").visible),
            "closed"
        );
        // Open again: Escape cancels.
        ui.open_popup(dialog);
        draw(&mut ui);
        super::key(&mut ui, "Escape");
        let said: Vec<FileDialogSignal> = ui
            .take_signals()
            .iter()
            .filter_map(|e| handle.signal(e))
            .filter(|s| {
                matches!(
                    s,
                    FileDialogSignal::Accepted(_) | FileDialogSignal::Rejected
                )
            })
            .collect();
        assert_eq!(said, [FileDialogSignal::Rejected]);
        assert!(
            !ui.get(ui.common(dialog).expect("common").visible),
            "closed"
        );
    }

    /// A root of `w` by `h` to put one control under, and a drawing of
    /// the window, which lays it out.
    fn stage(w: f32, h: f32) -> (tole::Ui, tole::handles::Item) {
        let mut ui = tole::Ui::new(super::fonts());
        let root = tole::handles::Item::new(&mut ui, None).expect("made");
        root.common.width.set(&mut ui, w);
        root.common.height.set(&mut ui, h);
        (ui, root)
    }

    fn draw(ui: &mut tole::Ui) {
        let _ = ui.render((1000, 700), tole::Color::TRANSPARENT);
    }

    /// The fill of the first `Rectangle` holding the `Text` that says
    /// `label`.
    fn fill_behind(ui: &mut tole::Ui, label: &str) -> tole::Color {
        let mut at = super::find_text(ui, label);
        while let Some(item) = at {
            if let Some(rect) = ui.rectangle(item) {
                return ui.get(rect.color);
            }
            at = ui.parent(item);
        }
        panic!("no rectangle behind {label}");
    }

    #[test]
    fn the_search_field_says_a_mode_picked_and_shows_the_mode_it_is_given() {
        use super::fx_controls::SearchFieldSignal;
        let (mut ui, root) = stage(600.0, 400.0);
        let field = super::fx_controls::SearchField::new(&mut ui, Some(root.id)).expect("made");
        draw(&mut ui);
        assert!(shown(&mut ui, "Filter by name"));
        // Typing is said, and the placeholder goes.
        super::press(&mut ui, "Filter by name", true).expect("clicked");
        let _ = ui.take_signals();
        type_text(&mut ui, "id_");
        draw(&mut ui);
        assert_eq!(&*ui.get(field.text), "id_");
        assert!(!shown(&mut ui, "Filter by name"));
        let said: Vec<SearchFieldSignal> = ui
            .take_signals()
            .iter()
            .filter_map(|e| field.signal(e))
            .collect();
        assert_eq!(
            said,
            [
                SearchFieldSignal::TextEdited,
                SearchFieldSignal::TextEdited,
                SearchFieldSignal::TextEdited
            ]
        );
        // A mode picked from its menu is said, not taken: the owner sets it.
        super::press(&mut ui, "Aa", true).expect("clicked");
        draw(&mut ui);
        super::press(&mut ui, "Advanced", true).expect("clicked");
        let said: Vec<SearchFieldSignal> = ui
            .take_signals()
            .iter()
            .filter_map(|e| field.signal(e))
            .collect();
        assert_eq!(said, [SearchFieldSignal::ModePicked(1)]);
        assert_eq!(ui.get(field.mode), 0);
        field.mode.set(&mut ui, 2);
        field.text.set(&mut ui, String::new().into());
        draw(&mut ui);
        assert!(shown(&mut ui, ".*"), "the button names the mode");
        assert!(shown(&mut ui, "^id_.*$"), "and so does the placeholder");
        // A search that cannot be read turns it red.
        let theme = super::fx_controls::Theme::instance(&mut ui);
        let calm = ui.get(field.base.props.color);
        assert_eq!(calm, ui.get(theme.bg2));
        field.error.set(&mut ui, "unclosed group".into());
        draw(&mut ui);
        assert_eq!(ui.get(field.base.props.color), ui.get(theme.red_bg));
    }

    #[test]
    fn breadcrumbs_name_the_crumb_clicked_and_mark_where_it_is() {
        use super::fx_controls::BreadcrumbsSignal;
        let (mut ui, root) = stage(600.0, 100.0);
        let bar = super::fx_controls::Breadcrumbs::new(&mut ui, Some(root.id)).expect("made");
        let crumbs = vec![
            tole::FileEntry::named("~", "/home/me"),
            tole::FileEntry::named("machines", "/home/me/machines"),
            tole::FileEntry::named("keys", "/home/me/machines/keys"),
        ];
        bar.crumbs
            .set(&mut ui, tole::ModelRc::new(tole::VecModel::new(crumbs)));
        bar.current.set(&mut ui, "/home/me/machines/keys".into());
        draw(&mut ui);
        assert!(shown(&mut ui, "machines"));
        let weight = |ui: &mut tole::Ui, label: &str| {
            let item = super::find_text(ui, label).expect("crumb");
            let text = ui.text(item).expect("text");
            ui.get(text.font).weight
        };
        assert_eq!(weight(&mut ui, "keys"), 500);
        assert_eq!(weight(&mut ui, "machines"), 400);
        let _ = ui.take_signals();
        super::press(&mut ui, "machines", true).expect("clicked");
        let said: Vec<BreadcrumbsSignal> = ui
            .take_signals()
            .iter()
            .filter_map(|e| bar.signal(e))
            .collect();
        assert_eq!(
            said,
            [BreadcrumbsSignal::CrumbClicked(
                1,
                "/home/me/machines".into()
            )]
        );
    }

    #[test]
    fn places_tint_where_it_is_and_name_the_place_clicked() {
        use super::fx_controls::PlacesListSignal;
        let (mut ui, root) = stage(300.0, 400.0);
        let places = super::fx_controls::PlacesList::new(&mut ui, Some(root.id)).expect("made");
        places
            .names
            .set(&mut ui, vec!["Home".into(), "Keys".into()]);
        places.paths.set(&mut ui, vec!["~".into(), "~/.ssh".into()]);
        places.current.set(&mut ui, "~/.ssh".into());
        draw(&mut ui);
        assert!(shown(&mut ui, "PLACES"));
        let theme = super::fx_controls::Theme::instance(&mut ui);
        assert_eq!(fill_behind(&mut ui, "Keys"), ui.get(theme.accent_bg));
        assert_eq!(fill_behind(&mut ui, "Home"), tole::Color::TRANSPARENT);
        let _ = ui.take_signals();
        super::press(&mut ui, "Home", true).expect("clicked");
        let said: Vec<PlacesListSignal> = ui
            .take_signals()
            .iter()
            .filter_map(|e| places.signal(e))
            .collect();
        assert_eq!(said, [PlacesListSignal::PlaceClicked(0, "~".into())]);
    }

    #[test]
    fn a_file_row_and_a_tile_say_clicks_and_show_a_picture_or_a_badge() {
        use super::fx_controls::{FileListRowSignal, FileTileSignal};
        let (mut ui, root) = stage(600.0, 400.0);
        let row = super::fx_controls::FileListRow::new(&mut ui, Some(root.id)).expect("made");
        row.base.common.width.set(&mut ui, 500.0);
        row.name.set(&mut ui, "id_ed25519".into());
        row.badge.set(&mut ui, "KEY".into());
        row.kind.set(&mut ui, "private".into());
        row.name_width.set(&mut ui, 300.0);
        draw(&mut ui);
        assert!(shown(&mut ui, "KEY"), "no picture: the badge");
        // The name ends where its width does; a mark after it goes there.
        assert_eq!(ui.get(row.name_end), 344.0);
        let narrow = ui.get(row.name_implicit_width);
        assert!(narrow > 0.0 && narrow < 300.0);
        let theme = super::fx_controls::Theme::instance(&mut ui);
        row.selected.set(&mut ui, true);
        draw(&mut ui);
        assert_eq!(ui.get(row.base.props.color), ui.get(theme.accent_over));
        let _ = ui.take_signals();
        for _ in 0..2 {
            super::press(&mut ui, "id_ed25519", true).expect("clicked");
        }
        let said: Vec<FileListRowSignal> = ui
            .take_signals()
            .iter()
            .filter_map(|e| row.signal(e))
            .collect();
        assert_eq!(
            said,
            [
                FileListRowSignal::Clicked,
                FileListRowSignal::DoubleClicked,
                FileListRowSignal::Clicked
            ]
        );
        // A picture in place of the badge.
        row.image.set(&mut ui, "/nowhere.png".into());
        draw(&mut ui);
        assert!(!shown(&mut ui, "KEY"));

        let tile = super::fx_controls::FileTile::new(&mut ui, Some(root.id)).expect("made");
        tile.base.common.y.set(&mut ui, 100.0);
        tile.name.set(&mut ui, "notes.txt".into());
        tile.size_text.set(&mut ui, "3 KB".into());
        tile.badge.set(&mut ui, "TXT".into());
        draw(&mut ui);
        assert!(shown(&mut ui, "TXT") && shown(&mut ui, "3 KB"));
        let _ = ui.take_signals();
        super::press(&mut ui, "notes.txt", true).expect("clicked");
        let said: Vec<FileTileSignal> = ui
            .take_signals()
            .iter()
            .filter_map(|e| tile.signal(e))
            .collect();
        assert_eq!(said, [FileTileSignal::Clicked]);
    }

    #[test]
    fn a_preview_shows_a_picture_or_text_and_says_where_it_ends() {
        let (mut ui, root) = stage(400.0, 500.0);
        let preview = super::fx_controls::FilePreview::new(&mut ui, Some(root.id)).expect("made");
        preview.base.common.height.set(&mut ui, 400.0);
        preview.name.set(&mut ui, "notes.txt".into());
        preview.kind_label.set(&mut ui, "Text".into());
        preview.text.set(&mut ui, "first line".into());
        draw(&mut ui);
        assert!(shown(&mut ui, "notes.txt") && shown(&mut ui, "Text"));
        assert!(super::find_input(&mut ui, "first line").is_some());
        assert_eq!(ui.get(preview.body_bottom), 66.0 + 180.0);
        preview.image.set(&mut ui, "/nowhere.png".into());
        preview.image_height.set(&mut ui, 200.0);
        draw(&mut ui);
        assert_eq!(ui.get(preview.body_bottom), 66.0 + 200.0);
    }

    #[test]
    fn the_pattern_help_says_why_over_the_syntax() {
        let (mut ui, root) = stage(600.0, 300.0);
        let help = super::fx_controls::PatternHelp::new(&mut ui, Some(root.id)).expect("made");
        help.base.common.width.set(&mut ui, 560.0);
        help.message.set(&mut ui, "Search: unclosed group".into());
        draw(&mut ui);
        assert!(shown(&mut ui, "Search: unclosed group"));
        assert!(shown(&mut ui, "? matches one character"));
    }

    #[test]
    fn the_small_tools_say_what_was_pressed_and_change_nothing_themselves() {
        use super::fx_controls::{
            CloseButtonSignal, SegmentedBarSignal, UpButtonSignal, ViewSwitchSignal,
        };
        let (mut ui, root) = stage(600.0, 400.0);
        let close = super::fx_controls::CloseButton::new(&mut ui, Some(root.id)).expect("made");
        let views = super::fx_controls::ViewSwitch::new(&mut ui, Some(root.id)).expect("made");
        views.base.common.x.set(&mut ui, 100.0);
        let up = super::fx_controls::UpButton::new(&mut ui, Some(root.id)).expect("made");
        up.base.common.x.set(&mut ui, 200.0);
        let bar = super::fx_controls::SegmentedBar::new(&mut ui, Some(root.id)).expect("made");
        bar.base.common.y.set(&mut ui, 100.0);
        bar.options.set(
            &mut ui,
            vec!["".into(), "All files".into(), "Custom".into()],
        );
        bar.current.set(&mut ui, 1);
        draw(&mut ui);
        let _ = ui.take_signals();
        super::click_at(&mut ui, 16.0, 16.0);
        super::click_at(&mut ui, 100.0 + 50.0, 19.0);
        super::click_at(&mut ui, 200.0 + 19.0, 19.0);
        let signals = ui.take_signals();
        let said = |f: &dyn Fn(&tole::Emitted) -> bool| signals.iter().filter(|e| f(e)).count();
        assert_eq!(
            said(&|e| close.signal(e) == Some(CloseButtonSignal::Clicked)),
            1
        );
        assert_eq!(
            said(&|e| views.signal(e) == Some(ViewSwitchSignal::GridPicked(true))),
            1
        );
        assert!(!ui.get(views.grid), "the owner sets it");
        assert_eq!(said(&|e| up.signal(e) == Some(UpButtonSignal::Clicked)), 1);
        // Nowhere to go: a press says nothing.
        up.can_go.set(&mut ui, false);
        draw(&mut ui);
        super::click_at(&mut ui, 200.0 + 19.0, 19.0);
        assert!(ui.take_signals().iter().all(|e| up.signal(e).is_none()));
        // An empty choice is left out; a press names the choice's index.
        assert!(super::find_text(&mut ui, "All files").is_some());
        super::press(&mut ui, "Custom", true).expect("clicked");
        let picked: Vec<SegmentedBarSignal> = ui
            .take_signals()
            .iter()
            .filter_map(|e| bar.signal(e))
            .collect();
        assert_eq!(picked, [SegmentedBarSignal::Picked(2)]);
        assert_eq!(ui.get(bar.current), 1);
        let all = super::find_text(&mut ui, "All files").expect("shown");
        let first = ui.rect_of(all).expect("laid out");
        assert!(first.x < 40.0, "the empty choice takes no room: {first:?}");
    }

    #[test]
    fn every_demo_draws_something() {
        for demo in &DEMOS {
            let png = render(demo, 1.0).expect("drawn");
            assert!(png.len() > 1000, "{}", demo.name);
        }
    }
}
