# tole

Ferrix's UI framework, after Qt Quick and QML: a retained tree of items
with typed properties and bindings, anchors and layouts, drawn on the CPU
with tiny-skia and Ferrix's own text, with a GPU path to come. Its markup,
`.tole` files, is compiled into Rust when a program is built, so a program
carries no interpreter.

The design, the plan and where it stands are in Ferrix's
[`docs/UI-FRAMEWORK.md`](https://github.com/ferrix-os/ferrix/blob/develop/docs/UI-FRAMEWORK.md).

## Crates

* **`tole`**: the runtime. Property cells and bindings with dependency
  tracking, the item tree (`Item`, `Rectangle`, `Text`, `Row`, `Column`),
  anchors, the scene as drawing operations, the `Renderer` trait and the
  CPU renderer, typed handles for each kind of item, and `include_ui!` for
  the compiler's output.
* **`tole-compiler`** (in progress): `.tole` files parsed, type-checked and
  written out as Rust from a program's `build.rs`.

## Building

tole is a component of [Ferrix](https://github.com/ferrix-os/ferrix): a
Ferrix tree checks it out at `src/user/system/linux/tole`
(`components.toml`), beside the compositor's workspace, and tole reaches
two of the compositor's crates by path from there: `text` (fonts, shaping
and glyphs, pure Rust over ttf-parser and rustybuzz) and, for the window
backend behind its `wayland` feature, `toolkit` (the Wayland client
runtime, pure Rust over the socket). Its tests draw with the tree's own
Inter from `assets/fonts`.

So to build it alone, put it where a Ferrix tree would:

```sh
git clone --filter=blob:none --sparse https://github.com/ferrix-os/ferrix
cd ferrix
git sparse-checkout set src/user/system/linux/compositor assets/fonts/inter
git clone https://github.com/ferrix-os/tole src/user/system/linux/tole
cd src/user/system/linux/tole
cargo test
```

which is what CI does. Inside a Ferrix tree, `cargo xtask check` runs
tole's formatting, clippy and tests with everything else.

## Changing it

A change to tole is two landings, as for every Ferrix component (Ferrix's
`docs/CONVENTIONS.md`, "Components live in repositories of their own"):
commit and push here, then pin the commit in Ferrix with `cargo xtask
pin-components` and land that. Commits name one author: no
`Co-authored-by:` trailer and no tool signature; CI refuses them.

## Licence

MIT, as Ferrix's compositor.
