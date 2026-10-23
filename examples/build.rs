//! The demos' markup, compiled into Rust.

fn main() {
    if tole_compiler::build("ui").is_err() {
        std::process::exit(1);
    }
}
