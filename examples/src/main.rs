//! `tole-examples NAME --png FILE` draws one demo; `tole-examples --all
//! DIR` draws every one into `DIR/NAME.png`, at scale 2; `tole-examples
//! NAME --window` opens it in a window on the Wayland desktop it runs on.

use std::io::Write as _;
use std::process::ExitCode;

use tole_examples::{DEMOS, render};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = std::io::stdout();
    let jobs: Vec<(&tole_examples::Demo, std::path::PathBuf)> = match args.as_slice() {
        [all, dir] if all == "--all" => DEMOS
            .iter()
            .map(|demo| {
                (
                    demo,
                    std::path::Path::new(dir).join(format!("{}.png", demo.name)),
                )
            })
            .collect(),
        [name, window] if window == "--window" => {
            return match DEMOS.iter().find(|demo| demo.name == name) {
                Some(demo) => match tole_examples::run_window(demo) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(error) => {
                        let _ = writeln!(out, "{name}: {error}");
                        ExitCode::FAILURE
                    }
                },
                None => {
                    let _ = writeln!(out, "no demo `{name}`");
                    ExitCode::FAILURE
                }
            };
        }
        [name, png, file] if png == "--png" => match DEMOS.iter().find(|demo| demo.name == name) {
            Some(demo) => vec![(demo, file.into())],
            None => {
                let _ = writeln!(out, "no demo `{name}`");
                return ExitCode::FAILURE;
            }
        },
        _ => {
            let _ = writeln!(
                out,
                "tole-examples NAME --png FILE | NAME --window | --all DIR"
            );
            for demo in &DEMOS {
                let _ = writeln!(out, "  {:10} {}", demo.name, demo.about);
            }
            return ExitCode::FAILURE;
        }
    };
    for (demo, path) in jobs {
        let Some(png) = render(demo, 2.0) else {
            let _ = writeln!(out, "{}: not drawn", demo.name);
            return ExitCode::FAILURE;
        };
        if let Err(error) = std::fs::write(&path, png) {
            let _ = writeln!(out, "{}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        let _ = writeln!(out, "{}: {}", demo.name, path.display());
    }
    ExitCode::SUCCESS
}
