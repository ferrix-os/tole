//! Host tests: bindings, the tree, positioners, anchors, text and frames.
//!
//! The faces are the tree's own Inter (`assets/fonts/inter`), so nothing
//! here depends on what the machine has installed.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::{Anchors, Color, DrawOp, Elide, Font, Fonts, HAlign, KindName, Line, Ui};

/// The tree's fonts directory.
fn tree_fonts() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../../../assets/fonts")
}

/// A user interface with Inter only.
fn ui() -> Ui {
    let mut fonts = Fonts::new();
    assert_eq!(fonts.add_dir(&tree_fonts().join("inter")), 2);
    Ui::new(fonts)
}

/// A user interface with no fonts at all.
fn bare() -> Ui {
    Ui::new(Fonts::new())
}

/// Inter at `px`.
fn inter(px: f32) -> Font {
    Font::new("Inter", px, 400)
}

#[test]
fn a_binding_follows_what_it_read() {
    let mut ui = bare();
    let a = ui.prop(2.0_f32);
    let b = ui.prop(3.0_f32);
    let sum = ui.prop(0.0_f32);
    let runs = Rc::new(Cell::new(0));
    let counted = Rc::clone(&runs);
    sum.bind(&mut ui, move |ui| {
        counted.set(counted.get() + 1);
        ui.get(a) + ui.get(b)
    });
    assert_eq!(runs.get(), 0, "a binding runs when read, not when made");
    assert_eq!(sum.get(&mut ui), 5.0);
    assert_eq!(sum.get(&mut ui), 5.0);
    assert_eq!(runs.get(), 1, "a clean binding is not run again");
    a.set(&mut ui, 10.0);
    b.set(&mut ui, 20.0);
    assert_eq!(runs.get(), 1, "setting only marks");
    assert_eq!(sum.get(&mut ui), 30.0);
    assert_eq!(runs.get(), 2, "two changes, one evaluation");
    b.set(&mut ui, 20.0);
    assert_eq!(sum.get(&mut ui), 30.0);
    assert_eq!(runs.get(), 2, "setting the same value changes nothing");
}

#[test]
fn a_binding_reads_through_other_bindings() {
    let mut ui = bare();
    let base = ui.prop(1_i32);
    let double = ui.prop(0_i32);
    let quad = ui.prop(0_i32);
    double.bind(&mut ui, move |ui| ui.get(base) * 2);
    quad.bind(&mut ui, move |ui| ui.get(double) * 2);
    assert_eq!(quad.get(&mut ui), 4);
    base.set(&mut ui, 5);
    assert_eq!(quad.get(&mut ui), 20);
}

#[test]
fn a_binding_drops_what_it_no_longer_reads() {
    let mut ui = bare();
    let which = ui.prop(true);
    let left = ui.prop(1_i32);
    let right = ui.prop(2_i32);
    let pick = ui.prop(0_i32);
    let runs = Rc::new(Cell::new(0));
    let counted = Rc::clone(&runs);
    pick.bind(&mut ui, move |ui| {
        counted.set(counted.get() + 1);
        if ui.get(which) {
            ui.get(left)
        } else {
            ui.get(right)
        }
    });
    assert_eq!(pick.get(&mut ui), 1);
    right.set(&mut ui, 3);
    assert_eq!(pick.get(&mut ui), 1);
    assert_eq!(
        runs.get(),
        1,
        "right was never read, so changing it costs nothing"
    );
    which.set(&mut ui, false);
    assert_eq!(pick.get(&mut ui), 3);
    left.set(&mut ui, 7);
    assert_eq!(pick.get(&mut ui), 3);
    assert_eq!(runs.get(), 2, "left is no longer read");
}

#[test]
fn setting_a_bound_property_drops_its_binding() {
    let mut ui = bare();
    let source = ui.prop(1_i32);
    let target = ui.prop(0_i32);
    target.bind(&mut ui, move |ui| ui.get(source));
    assert_eq!(target.get(&mut ui), 1);
    target.set(&mut ui, 9);
    source.set(&mut ui, 2);
    assert_eq!(target.get(&mut ui), 9);
}

#[test]
fn a_binding_loop_is_found_and_ends() {
    let mut ui = bare();
    let a = ui.prop(1_i32);
    let b = ui.prop(1_i32);
    a.bind(&mut ui, move |ui| ui.get(b) + 1);
    b.bind(&mut ui, move |ui| ui.get(a) + 1);
    let _ = a.get(&mut ui);
    assert_eq!(ui.binding_loops().len(), 1);
    let _ = b.get(&mut ui);
    assert_eq!(ui.binding_loops().len(), 1, "a loop is said once");
}

#[test]
fn width_follows_the_implicit_width_until_set() {
    let mut ui = bare();
    let item = ui.create(KindName::Item, None).unwrap();
    let c = ui.common(item).unwrap();
    c.implicit_width.set(&mut ui, 40.0);
    assert_eq!(c.width.get(&mut ui), 40.0);
    c.width.set(&mut ui, 25.0);
    c.implicit_width.set(&mut ui, 80.0);
    assert_eq!(c.width.get(&mut ui), 25.0);
}

#[test]
fn a_row_places_its_children_and_sizes_itself() {
    let mut ui = bare();
    let row = ui.create(KindName::Row, None).unwrap();
    let p = ui.positioner(row).unwrap();
    p.spacing.set(&mut ui, 8.0);
    p.padding.set(&mut ui, 2.0);
    let sizes = [(10.0, 5.0), (20.0, 30.0), (30.0, 10.0)];
    let mut kids = Vec::new();
    for (w, h) in sizes {
        let kid = ui.create(KindName::Rectangle, Some(row)).unwrap();
        let c = ui.common(kid).unwrap();
        c.width.set(&mut ui, w);
        c.height.set(&mut ui, h);
        kids.push(c);
    }
    let r = ui.common(row).unwrap();
    let xs: Vec<f32> = kids.iter().map(|c| c.x.get(&mut ui)).collect();
    assert_eq!(xs, [2.0, 20.0, 48.0]);
    assert_eq!(
        r.width.get(&mut ui),
        2.0 + 10.0 + 8.0 + 20.0 + 8.0 + 30.0 + 2.0
    );
    assert_eq!(r.height.get(&mut ui), 34.0);
    // An invisible child takes no room.
    kids[1].visible.set(&mut ui, false);
    assert_eq!(kids[2].x.get(&mut ui), 20.0);
    assert_eq!(r.width.get(&mut ui), 2.0 + 10.0 + 8.0 + 30.0 + 2.0);
    assert_eq!(r.height.get(&mut ui), 14.0);
    // A width changed moves what follows.
    kids[0].width.set(&mut ui, 50.0);
    assert_eq!(kids[2].x.get(&mut ui), 60.0);
}

#[test]
fn a_column_stacks_and_a_destroyed_child_closes_up() {
    let mut ui = bare();
    let column = ui.create(KindName::Column, None).unwrap();
    ui.positioner(column).unwrap().spacing.set(&mut ui, 4.0);
    let mut ids = Vec::new();
    for h in [10.0, 20.0, 30.0] {
        let kid = ui.create(KindName::Item, Some(column)).unwrap();
        ui.common(kid).unwrap().height.set(&mut ui, h);
        ids.push(kid);
    }
    let last = ui.common(ids[2]).unwrap();
    assert_eq!(last.y.get(&mut ui), 38.0);
    ui.destroy(ids[1]);
    assert!(!ui.exists(ids[1]));
    assert!(
        ui.common(ids[1]).is_none(),
        "a destroyed item's id is refused"
    );
    assert_eq!(last.y.get(&mut ui), 14.0);
    assert_eq!(ui.common(column).unwrap().height.get(&mut ui), 44.0);
    // The slot is taken again; the old id still names nothing.
    let again = ui.create(KindName::Item, Some(column)).unwrap();
    assert_ne!(again, ids[1]);
    assert!(ui.common(ids[1]).is_none());
}

#[test]
fn anchors_fill_center_and_pin_to_a_sibling() {
    let mut ui = bare();
    let root = ui.create(KindName::Item, None).unwrap();
    let rc = ui.common(root).unwrap();
    rc.width.set(&mut ui, 200.0);
    rc.height.set(&mut ui, 100.0);

    let fill = ui.create(KindName::Rectangle, Some(root)).unwrap();
    assert!(ui.anchor(
        fill,
        &Anchors {
            fill: Some(root),
            margins: 10.0,
            left_margin: Some(20.0),
            ..Anchors::default()
        }
    ));
    let f = ui.common(fill).unwrap();
    assert_eq!((f.x.get(&mut ui), f.y.get(&mut ui)), (20.0, 10.0));
    assert_eq!((f.width.get(&mut ui), f.height.get(&mut ui)), (170.0, 80.0));

    let centred = ui.create(KindName::Rectangle, Some(root)).unwrap();
    let c = ui.common(centred).unwrap();
    c.width.set(&mut ui, 40.0);
    c.height.set(&mut ui, 20.0);
    assert!(ui.anchor(
        centred,
        &Anchors {
            center_in: Some(root),
            ..Anchors::default()
        }
    ));
    assert_eq!((c.x.get(&mut ui), c.y.get(&mut ui)), (80.0, 40.0));

    // Beside the centred one, right edge to its left edge.
    let beside = ui.create(KindName::Rectangle, Some(root)).unwrap();
    let b = ui.common(beside).unwrap();
    b.width.set(&mut ui, 30.0);
    assert!(ui.anchor(
        beside,
        &Anchors {
            right: Some((centred, Line::Left)),
            right_margin: Some(5.0),
            ..Anchors::default()
        }
    ));
    assert_eq!(b.x.get(&mut ui), 45.0);

    // The window grows: everything follows.
    rc.width.set(&mut ui, 400.0);
    assert_eq!(f.width.get(&mut ui), 370.0);
    assert_eq!(c.x.get(&mut ui), 180.0);
    assert_eq!(b.x.get(&mut ui), 145.0);

    // Anchoring to an item that is neither parent nor sibling is refused.
    let stranger = ui.create(KindName::Item, None).unwrap();
    assert!(!ui.anchor(
        beside,
        &Anchors {
            left: Some((stranger, Line::Left)),
            ..Anchors::default()
        }
    ));
}

#[test]
fn text_measures_itself_and_follows_its_text_and_scale() {
    let mut ui = ui();
    let label = ui.create(KindName::Text, None).unwrap();
    let t = ui.text(label).unwrap();
    let c = ui.common(label).unwrap();
    t.font.set(&mut ui, inter(20.0));
    t.text.set(&mut ui, Rc::from("Hello"));
    let short = c.width.get(&mut ui);
    assert!(short > 30.0 && short < 80.0, "{short}");
    let height = c.height.get(&mut ui);
    assert!(height > 18.0 && height < 30.0, "{height}");
    t.text.set(&mut ui, Rc::from("Hello, world"));
    let long = c.width.get(&mut ui);
    assert!(long > short * 2.0, "{long} against {short}");
    // At twice the scale, the logical width stays (within hinting).
    ui.set_scale(2.0);
    let scaled = c.width.get(&mut ui);
    assert!((scaled - long).abs() <= 1.5, "{scaled} against {long}");
}

#[test]
fn a_row_of_text_moves_when_a_label_changes() {
    let mut ui = ui();
    let row = ui.create(KindName::Row, None).unwrap();
    let first = ui.create(KindName::Text, Some(row)).unwrap();
    let second = ui.create(KindName::Text, Some(row)).unwrap();
    let (t1, t2) = (ui.text(first).unwrap(), ui.text(second).unwrap());
    t1.text.set(&mut ui, Rc::from("a"));
    t2.text.set(&mut ui, Rc::from("b"));
    let x2 = ui.common(second).unwrap().x;
    let before = x2.get(&mut ui);
    t1.text.set(&mut ui, Rc::from("a much longer label"));
    assert!(x2.get(&mut ui) > before + 50.0);
}

/// A pixel of `pixmap`, premultiplied RGBA.
fn pixel(pixmap: &tiny_skia::Pixmap, x: u32, y: u32) -> [u8; 4] {
    let p = pixmap.pixel(x, y).unwrap();
    [p.red(), p.green(), p.blue(), p.alpha()]
}

#[test]
fn a_frame_draws_boxes_borders_and_cuts() {
    let mut ui = bare();
    let root = ui.create(KindName::Rectangle, None).unwrap();
    let rc = ui.common(root).unwrap();
    rc.width.set(&mut ui, 40.0);
    rc.height.set(&mut ui, 30.0);
    let r = ui.rectangle(root).unwrap();
    r.color.set(&mut ui, Color::hex(0x00_00_ff));
    r.border_width.set(&mut ui, 2.0);
    r.border_color.set(&mut ui, Color::hex(0xff_00_00));
    rc.clip.set(&mut ui, true);
    // A child hanging past the right edge, cut there.
    let kid = ui.create(KindName::Rectangle, Some(root)).unwrap();
    let kc = ui.common(kid).unwrap();
    kc.x.set(&mut ui, 30.0);
    kc.y.set(&mut ui, 10.0);
    kc.width.set(&mut ui, 30.0);
    kc.height.set(&mut ui, 10.0);
    ui.rectangle(kid)
        .unwrap()
        .color
        .set(&mut ui, Color::hex(0x00_ff_00));

    let frame = ui.render((60, 40), Color::hex(0xff_ff_ff)).unwrap();
    assert_eq!(pixel(&frame, 10, 15), [0, 0, 255, 255], "the fill");
    assert_eq!(
        pixel(&frame, 0, 15),
        [255, 0, 0, 255],
        "the border, inside the edge"
    );
    assert_eq!(pixel(&frame, 35, 15), [0, 255, 0, 255], "the child");
    assert_eq!(
        pixel(&frame, 45, 15),
        [255, 255, 255, 255],
        "the child past the cut"
    );
    assert_eq!(
        pixel(&frame, 50, 35),
        [255, 255, 255, 255],
        "outside the root"
    );

    // At scale 2 the same places are twice as far.
    ui.set_scale(2.0);
    let frame = ui.render((60, 40), Color::hex(0xff_ff_ff)).unwrap();
    assert_eq!((frame.width(), frame.height()), (120, 80));
    assert_eq!(pixel(&frame, 70, 30), [0, 255, 0, 255]);
    assert_eq!(pixel(&frame, 90, 30), [255, 255, 255, 255]);
}

#[test]
fn opacity_draws_a_subtree_together() {
    let mut ui = bare();
    let root = ui.create(KindName::Rectangle, None).unwrap();
    let rc = ui.common(root).unwrap();
    rc.width.set(&mut ui, 10.0);
    rc.height.set(&mut ui, 10.0);
    rc.opacity.set(&mut ui, 0.5);
    ui.rectangle(root)
        .unwrap()
        .color
        .set(&mut ui, Color::hex(0));
    // A child over the parent: together they are one black square at half,
    // not a darker square where they overlap.
    let kid = ui.create(KindName::Rectangle, Some(root)).unwrap();
    let kc = ui.common(kid).unwrap();
    kc.width.set(&mut ui, 5.0);
    kc.height.set(&mut ui, 5.0);
    ui.rectangle(kid).unwrap().color.set(&mut ui, Color::hex(0));
    let frame = ui.render((10, 10), Color::hex(0xff_ff_ff)).unwrap();
    let (a, b) = (pixel(&frame, 2, 2), pixel(&frame, 7, 7));
    assert_eq!(a, b);
    assert!((126..=129).contains(&a[0]), "{a:?}");
}

#[test]
fn text_is_placed_elided_and_cut() {
    let mut ui = ui();
    let label = ui.create(KindName::Text, None).unwrap();
    let t = ui.text(label).unwrap();
    let c = ui.common(label).unwrap();
    t.font.set(&mut ui, inter(16.0));
    t.text
        .set(&mut ui, Rc::from("A label that is far too long to fit"));
    t.color.set(&mut ui, Color::hex(0));
    c.width.set(&mut ui, 60.0);
    c.height.set(&mut ui, 24.0);
    t.elide.set(&mut ui, Elide::Right);
    let ops = ui.scene();
    let [
        DrawOp::Text {
            run, x, baseline, ..
        },
    ] = ops.as_slice()
    else {
        panic!("one run: {ops:?}");
    };
    assert!(run.width <= 60.0, "elided to the item: {}", run.width);
    assert_eq!(*x, 0.0);
    assert!(*baseline > 12.0 && *baseline < 24.0, "{baseline}");

    t.elide.set(&mut ui, Elide::None);
    t.horizontal_alignment.set(&mut ui, HAlign::Right);
    t.text.set(&mut ui, Rc::from("ok"));
    let ops = ui.scene();
    let [DrawOp::Text { run, x, .. }] = ops.as_slice() else {
        panic!("one run: {ops:?}");
    };
    assert!(
        (x + run.width - 60.0).abs() <= 1.0,
        "right-aligned: {x} + {}",
        run.width
    );

    // Drawn: dark pixels inside the item, none outside.
    let frame = ui.render((80, 24), Color::hex(0xff_ff_ff)).unwrap();
    let dark = |x0: u32, x1: u32| {
        (x0..x1)
            .flat_map(|x| (0..24).map(move |y| (x, y)))
            .filter(|&(x, y)| pixel(&frame, x, y)[0] < 128)
            .count()
    };
    assert!(dark(40, 60) > 10);
    assert_eq!(dark(60, 80), 0);
}

#[test]
fn a_frame_is_a_png() {
    let mut ui = bare();
    let root = ui.create(KindName::Rectangle, None).unwrap();
    let rc = ui.common(root).unwrap();
    rc.width.set(&mut ui, 3.0);
    rc.height.set(&mut ui, 2.0);
    ui.rectangle(root)
        .unwrap()
        .color
        .set(&mut ui, Color::rgba(255, 0, 0, 128));
    let frame = ui.render((3, 2), Color::TRANSPARENT).unwrap();
    let png = crate::png::encode(frame.width(), frame.height(), frame.data());
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&png[12..16], b"IHDR");
    assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    // The image data, inflated: three pixels a row, each row after a
    // filter byte.
    let at = png.windows(4).position(|w| w == b"IDAT").unwrap();
    let len = u32::from_be_bytes(png[at - 4..at].try_into().unwrap()) as usize;
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&png[at + 4..at + 4 + len]).unwrap();
    assert_eq!(raw.len(), 2 * (1 + 3 * 4));
    assert_eq!(raw[0], 0, "filter byte");
    assert_eq!(&raw[1..5], &[255, 0, 0, 128], "straight alpha again");
}

#[test]
fn handles_make_items_of_their_kind() {
    use crate::Element;
    let mut ui = bare();
    let column = crate::handles::Column::new(&mut ui, None).unwrap();
    let rect = crate::handles::Rectangle::new(&mut ui, Some(column.id())).unwrap();
    let label = crate::handles::Text::new(&mut ui, Some(column.id)).unwrap();
    assert_eq!(ui.children(column.id), [rect.id, label.id]);
    rect.common().height.set(&mut ui, 12.0);
    column.props.spacing.set(&mut ui, 3.0);
    assert_eq!(label.common.y.get(&mut ui), 15.0);
    let root = ui.roots()[0];
    assert!(crate::handles::Text::new(&mut ui, Some(root)).is_some());
    ui.destroy(column.id);
    assert!(
        crate::handles::Item::new(&mut ui, Some(column.id)).is_none(),
        "no item under a destroyed parent"
    );
}

#[test]
fn a_declared_type_is_a_property_value() {
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Line {
        level: i32,
        text: String,
    }
    impl crate::PropType for Line {
        fn into_value(self) -> crate::Value {
            crate::Value::custom(self)
        }
        fn from_value(value: &crate::Value) -> Option<Self> {
            value.downcast::<Self>().cloned()
        }
        fn fallback() -> Self {
            Self::default()
        }
    }
    let mut ui = bare();
    let line = ui.prop(Line {
        level: 3,
        text: "err".to_owned(),
    });
    let level = ui.prop(0_i32);
    let runs = Rc::new(Cell::new(0));
    let counted = Rc::clone(&runs);
    level.bind(&mut ui, move |ui| {
        counted.set(counted.get() + 1);
        ui.get(line).level
    });
    assert_eq!(level.get(&mut ui), 3);
    line.set(
        &mut ui,
        Line {
            level: 3,
            text: "err".to_owned(),
        },
    );
    assert_eq!(level.get(&mut ui), 3);
    assert_eq!(runs.get(), 1, "an equal value changes nothing");
    line.set(
        &mut ui,
        Line {
            level: 4,
            text: "warn".to_owned(),
        },
    );
    assert_eq!(level.get(&mut ui), 4);
    assert_ne!(
        crate::Value::custom(1_u8),
        crate::Value::custom(1_u16),
        "types differ"
    );
}

/// A mouse area of `w` × `h` at (`x`, `y`) under `parent`.
fn area(
    ui: &mut Ui,
    parent: crate::ItemId,
    (x, y, w, h): (f32, f32, f32, f32),
) -> crate::handles::MouseArea {
    let a = crate::handles::MouseArea::new(ui, Some(parent)).unwrap();
    a.common.x.set(ui, x);
    a.common.y.set(ui, y);
    a.common.width.set(ui, w);
    a.common.height.set(ui, h);
    a
}

/// The signals emitted since the last call, by id.
fn emitted(ui: &mut Ui) -> Vec<crate::SignalId> {
    ui.take_signals().into_iter().map(|e| e.signal).collect()
}

#[test]
fn a_press_and_release_is_a_click_on_the_topmost_area() {
    let mut ui = bare();
    let root = crate::handles::Item::new(&mut ui, None).unwrap();
    let below = area(&mut ui, root.id, (0.0, 0.0, 100.0, 100.0));
    let above = area(&mut ui, root.id, (50.0, 50.0, 100.0, 100.0));
    ui.pointer_move(75.0, 75.0);
    ui.pointer_press();
    assert!(above.props.pressed.get(&mut ui));
    assert!(!below.props.pressed.get(&mut ui));
    ui.pointer_release();
    assert_eq!(
        emitted(&mut ui),
        [
            above.props.on_pressed,
            above.props.released,
            above.props.clicked
        ]
    );
    // Only the lower one is there.
    ui.pointer_move(20.0, 20.0);
    ui.pointer_press();
    ui.pointer_release();
    assert_eq!(
        emitted(&mut ui),
        [
            below.props.on_pressed,
            below.props.released,
            below.props.clicked
        ]
    );
}

#[test]
fn a_press_holds_the_pointer_and_a_release_elsewhere_is_no_click() {
    let mut ui = bare();
    let root = crate::handles::Item::new(&mut ui, None).unwrap();
    let a = area(&mut ui, root.id, (0.0, 0.0, 50.0, 50.0));
    let other = area(&mut ui, root.id, (100.0, 0.0, 50.0, 50.0));
    ui.pointer_move(10.0, 10.0);
    ui.pointer_press();
    ui.pointer_move(120.0, 10.0);
    assert_eq!(
        a.props.mouse_x.get(&mut ui),
        120.0,
        "the held area follows the pointer"
    );
    ui.pointer_release();
    let got = emitted(&mut ui);
    assert_eq!(
        got,
        [
            a.props.on_pressed,
            a.props.position_changed,
            a.props.released
        ],
        "a move while held says so"
    );
    assert!(!got.contains(&other.props.clicked));
}

#[test]
fn hover_enters_and_exits_and_a_clip_hides_what_it_cuts() {
    let mut ui = bare();
    let root = crate::handles::Rectangle::new(&mut ui, None).unwrap();
    root.common.width.set(&mut ui, 100.0);
    root.common.height.set(&mut ui, 100.0);
    root.common.clip.set(&mut ui, true);
    let a = area(&mut ui, root.id, (80.0, 0.0, 50.0, 50.0));
    a.props.hover_enabled.set(&mut ui, true);
    ui.pointer_move(90.0, 10.0);
    assert!(a.props.contains_mouse.get(&mut ui));
    assert_eq!(emitted(&mut ui), [a.props.entered]);
    assert_eq!(a.props.mouse_x.get(&mut ui), 10.0);
    // Past the clip: gone.
    ui.pointer_move(110.0, 10.0);
    assert!(!a.props.contains_mouse.get(&mut ui));
    assert_eq!(emitted(&mut ui), [a.props.exited]);
    // Without hover, nothing is said.
    a.props.hover_enabled.set(&mut ui, false);
    ui.pointer_move(90.0, 10.0);
    assert!(emitted(&mut ui).is_empty());
    // A disabled area takes nothing.
    a.common.enabled.set(&mut ui, false);
    ui.pointer_press();
    ui.pointer_release();
    assert!(emitted(&mut ui).is_empty());
}

#[test]
fn handlers_run_in_order_and_signals_die_with_their_item() {
    let mut ui = bare();
    let root = crate::handles::Item::new(&mut ui, None).unwrap();
    let count = ui.own_prop(root.id, 0_i32);
    let said = ui.own_signal(root.id);
    ui.connect(
        said,
        Rc::new(move |ui: &mut Ui, args: &[crate::Value]| {
            let by = args
                .first()
                .and_then(<i32 as crate::PropType>::from_value)
                .unwrap_or(0);
            let now = count.get(ui);
            count.set(ui, now + by);
        }),
    );
    ui.emit(said, vec![crate::Value::Int(3)]);
    ui.emit(said, vec![crate::Value::Int(4)]);
    assert_eq!(count.get(&mut ui), 7);
    assert_eq!(ui.take_signals().len(), 2);
    ui.destroy(root.id);
    assert!(
        !ui.is_live(count.id),
        "a declared property goes with its item"
    );
    ui.emit(said, vec![crate::Value::Int(1)]);
    assert!(
        ui.take_signals().is_empty(),
        "and so does a declared signal"
    );
}

#[test]
fn a_handler_emitting_itself_stops() {
    let mut ui = bare();
    let root = crate::handles::Item::new(&mut ui, None).unwrap();
    let said = ui.own_signal(root.id);
    ui.connect(
        said,
        Rc::new(move |ui: &mut Ui, _: &[crate::Value]| ui.emit(said, Vec::new())),
    );
    ui.emit(said, Vec::new());
    assert_eq!(ui.take_signals().len(), 64);
}

/// A small scene for the damage tests: a white root, a box and a label
/// in a clipped column.
fn damage_scene(
    ui: &mut Ui,
) -> (
    crate::handles::Rectangle,
    crate::handles::Rectangle,
    crate::handles::Text,
) {
    let root = crate::handles::Rectangle::new(ui, None).unwrap();
    root.common.width.set(ui, 200.0);
    root.common.height.set(ui, 120.0);
    let boxed = crate::handles::Rectangle::new(ui, Some(root.id)).unwrap();
    boxed.common.x.set(ui, 10.0);
    boxed.common.y.set(ui, 10.0);
    boxed.common.width.set(ui, 40.0);
    boxed.common.height.set(ui, 30.0);
    boxed.props.radius.set(ui, 6.0);
    boxed.props.color.set(ui, Color::hex(0x2a_62_c9));
    let column = crate::handles::Column::new(ui, Some(root.id)).unwrap();
    column.common.x.set(ui, 70.0);
    column.common.y.set(ui, 20.0);
    column.common.clip.set(ui, true);
    let label = crate::handles::Text::new(ui, Some(column.id)).unwrap();
    label.props.font.set(ui, inter(14.0));
    label.props.color.set(ui, Color::hex(0x1a_1a_1a));
    label.props.text.set(ui, Rc::from("damage"));
    (root, boxed, label)
}

/// A change made to a scene.
type Step = Box<dyn Fn(&mut Ui)>;

#[test]
fn a_kept_frame_redraws_only_what_changed_and_matches_a_whole_one() {
    let mut ui = ui();
    ui.set_scale(2.0);
    let (root, boxed, label) = damage_scene(&mut ui);
    let mut frame = crate::Frame::new((200, 120), 2.0, Color::hex(0xff_ff_ff)).unwrap();
    assert_eq!(
        ui.render_into(&mut frame),
        [(0, 0, 400, 240)],
        "the first draw is whole"
    );
    assert!(
        ui.render_into(&mut frame).is_empty(),
        "nothing changed, nothing drawn"
    );

    // One box's colour: its place, a pixel round it, and nothing else.
    boxed.props.color.set(&mut ui, Color::hex(0xd2_38_2b));
    let damage = ui.render_into(&mut frame);
    assert_eq!(
        damage,
        [(18, 18, 84, 64)],
        "the box at scale 2, a logical pixel round it"
    );

    // A sequence of changes; after each, the kept frame must be what a
    // whole frame of the same scene is.
    let steps: Vec<Step> = vec![
        Box::new(move |ui| boxed.common.x.set(ui, 120.0)),
        Box::new(move |ui| label.props.text.set(ui, Rc::from("a longer label now"))),
        Box::new(move |ui| boxed.common.visible.set(ui, false)),
        Box::new(move |ui| boxed.common.visible.set(ui, true)),
        Box::new(move |ui| boxed.common.opacity.set(ui, 0.5)),
        Box::new(move |ui| {
            let extra = crate::handles::Rectangle::new(ui, Some(root.id)).unwrap();
            extra.common.width.set(ui, 30.0);
            extra.common.height.set(ui, 30.0);
            extra.props.color.set(ui, Color::hex(0x2f_a3_5a));
        }),
        Box::new(move |ui| ui.destroy(boxed.id)),
    ];
    for (index, step) in steps.iter().enumerate() {
        step(&mut ui);
        let damage = ui.render_into(&mut frame);
        assert!(!damage.is_empty(), "step {index} damaged something");
        let whole = ui.render((200, 120), Color::hex(0xff_ff_ff)).unwrap();
        let differing = whole
            .data()
            .chunks_exact(4)
            .zip(frame.pixmap().data().chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            differing, 0,
            "step {index}: the kept frame differs from a whole one in {differing} pixels"
        );
    }
}

#[test]
fn a_kept_frame_matches_a_whole_one_as_a_rounded_box_wanders() {
    for scale in [1.0, 1.5, 2.0] {
        let mut ui = ui();
        ui.set_scale(scale);
        let (_, boxed, label) = damage_scene(&mut ui);
        boxed.props.radius.set(&mut ui, 11.0);
        boxed.props.border_width.set(&mut ui, 1.5);
        boxed
            .props
            .border_color
            .set(&mut ui, Color::hex(0x1a_1a_1a));
        let mut frame = crate::Frame::new((200, 120), scale, Color::hex(0xff_ff_ff)).unwrap();
        let _ = ui.render_into(&mut frame);
        for step in 0..40_u16 {
            let t = f32::from(step);
            boxed.common.x.set(&mut ui, (t * 7.3) % 170.0);
            boxed.common.y.set(&mut ui, (t * 4.1) % 95.0);
            if step % 5 == 0 {
                label
                    .props
                    .text
                    .set(&mut ui, Rc::from(format!("step {step}")));
            }
            let _ = ui.render_into(&mut frame);
            let whole = ui.render((200, 120), Color::hex(0xff_ff_ff)).unwrap();
            let worst = whole
                .data()
                .iter()
                .zip(frame.pixmap().data())
                .map(|(a, b)| a.abs_diff(*b))
                .max();
            assert_eq!(worst, Some(0), "scale {scale}, step {step}");
        }
    }
}

/// A plain item under `parent` that Tab stops at.
fn stop(ui: &mut Ui, parent: crate::ItemId) -> crate::handles::Item {
    let item = crate::handles::Item::new(ui, Some(parent)).unwrap();
    item.common.active_focus_on_tab.set(ui, true);
    item
}

#[test]
fn focus_moves_within_a_scope_and_a_scope_keeps_its_own() {
    let mut ui = bare();
    let root = crate::handles::Item::new(&mut ui, None).unwrap();
    let a = crate::handles::Item::new(&mut ui, Some(root.id)).unwrap();
    let scope = crate::handles::FocusScope::new(&mut ui, Some(root.id)).unwrap();
    let inner = crate::handles::Item::new(&mut ui, Some(scope.id)).unwrap();
    let other = crate::handles::Item::new(&mut ui, Some(scope.id)).unwrap();

    ui.set_focus(a.id, true);
    assert!(ui.get(a.common.focus) && ui.get(a.common.active_focus));
    assert_eq!(ui.active_focus_item(), Some(a.id));

    // Focus inside the scope is kept there, but keys still go to `a`.
    ui.set_focus(inner.id, true);
    assert!(ui.get(inner.common.focus));
    assert!(!ui.get(inner.common.active_focus));
    assert_eq!(ui.active_focus_item(), Some(a.id));

    // The scope takes the focus from `a`, and its own focused item has
    // the active focus with it.
    ui.set_focus(scope.id, true);
    assert!(!ui.get(a.common.focus) && !ui.get(a.common.active_focus));
    assert!(ui.get(scope.common.active_focus) && ui.get(inner.common.active_focus));
    assert_eq!(ui.active_focus_item(), Some(inner.id));

    // Within it, one at a time.
    ui.set_focus(other.id, true);
    assert!(!ui.get(inner.common.focus) && !ui.get(inner.common.active_focus));
    assert_eq!(ui.active_focus_item(), Some(other.id));

    // forceActiveFocus reaches through every scope.
    ui.force_active_focus(a.id);
    assert_eq!(ui.active_focus_item(), Some(a.id));
    ui.force_active_focus(inner.id);
    assert_eq!(ui.active_focus_item(), Some(inner.id));
    assert!(ui.get(scope.common.focus));

    // The window losing the keyboard takes the active focus, not the
    // focus.
    ui.set_window_active(false);
    assert!(!ui.get(inner.common.active_focus) && ui.get(inner.common.focus));
    ui.set_window_active(true);
    assert!(ui.get(inner.common.active_focus));

    // An item that goes takes its active focus with it.
    ui.destroy(inner.id);
    assert_eq!(ui.active_focus_item(), Some(scope.id));
    assert!(ui.get(scope.common.active_focus));
}

#[test]
fn keys_go_to_the_focus_then_up_until_one_accepts() {
    let mut ui = bare();
    let root = crate::handles::Item::new(&mut ui, None).unwrap();
    let field = crate::handles::Item::new(&mut ui, Some(root.id)).unwrap();
    let heard = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));

    // The root hears everything that bubbles up to it.
    let on_root = ui.keys_pressed(root.id).unwrap();
    let log = Rc::clone(&heard);
    ui.connect(
        on_root,
        Rc::new(move |_ui: &mut Ui, args: &[crate::Value]| {
            let event = args[0].downcast::<crate::KeyEvent>().unwrap();
            log.borrow_mut().push(format!("root {}", event.key));
        }),
    );
    // The field takes letters by accepting them, and Return by its own
    // handler.
    let on_field = ui.keys_pressed(field.id).unwrap();
    let log = Rc::clone(&heard);
    ui.connect(
        on_field,
        Rc::new(move |ui: &mut Ui, args: &[crate::Value]| {
            let event = args[0].downcast::<crate::KeyEvent>().unwrap();
            log.borrow_mut().push(format!("field {}", event.key));
            if !event.text.is_empty() {
                ui.accept_key(true);
            }
        }),
    );
    let on_return = ui.keys_key_pressed(field.id, "Return").unwrap();
    let log = Rc::clone(&heard);
    ui.connect(
        on_return,
        Rc::new(move |_ui: &mut Ui, _args: &[crate::Value]| {
            log.borrow_mut().push("field Return!".to_owned());
        }),
    );

    // Nothing has the focus: keys go to the root.
    assert!(!ui.key_press(&crate::KeyEvent::named("a")));
    ui.set_focus(field.id, true);
    assert!(ui.key_press(&crate::KeyEvent::named("a")));
    assert!(ui.key_press(&crate::KeyEvent::named("Return")));
    assert!(!ui.key_press(&crate::KeyEvent::named("Escape")));
    assert_eq!(
        *heard.borrow(),
        [
            "root a",
            "field a",
            "field Return!",
            "field Escape",
            "root Escape"
        ]
    );
}

#[test]
fn tab_walks_the_stops_in_tree_order_and_key_navigation_jumps() {
    let mut ui = bare();
    let root = crate::handles::Item::new(&mut ui, None).unwrap();
    let a = stop(&mut ui, root.id);
    let group = crate::handles::Item::new(&mut ui, Some(root.id)).unwrap();
    let b = stop(&mut ui, group.id);
    let hidden = stop(&mut ui, group.id);
    hidden.common.visible.set(&mut ui, false);
    let c = stop(&mut ui, root.id);
    let disabled = stop(&mut ui, root.id);
    disabled.common.enabled.set(&mut ui, false);
    let tab = crate::KeyEvent::named("Tab");
    let backtab = crate::KeyEvent::named("ISO_Left_Tab");

    let mut order = Vec::new();
    for _ in 0..4 {
        assert!(ui.key_press(&tab));
        order.push(ui.active_focus_item().unwrap());
    }
    assert_eq!(order, [a.id, b.id, c.id, a.id], "in tree order, wrapping");
    assert!(ui.key_press(&backtab));
    assert_eq!(ui.active_focus_item(), Some(c.id));
    assert!(ui.key_press(&crate::KeyEvent::named("Tab").shifted()));
    assert_eq!(ui.active_focus_item(), Some(b.id));
    assert!(ui.get(b.common.active_focus) && !ui.get(c.common.active_focus));

    // KeyNavigation goes where it says, Tab included.
    ui.set_key_navigation(b.id, crate::Nav::Down, a.id);
    ui.set_key_navigation(b.id, crate::Nav::Tab, a.id);
    assert!(ui.key_press(&crate::KeyEvent::named("Down")));
    assert_eq!(ui.active_focus_item(), Some(a.id));
    ui.force_active_focus(b.id);
    assert!(ui.key_press(&tab));
    assert_eq!(ui.active_focus_item(), Some(a.id));
    assert!(!ui.key_press(&crate::KeyEvent::named("Up")), "a has none");
}

/// A rectangle under `parent` asking for `w` × `h`.
fn sized(ui: &mut Ui, parent: crate::ItemId, w: f32, h: f32) -> crate::handles::Rectangle {
    let r = crate::handles::Rectangle::new(ui, Some(parent)).unwrap();
    r.common.implicit_width.set(ui, w);
    r.common.implicit_height.set(ui, h);
    r
}

/// Where `item` is, from its parent, and its size.
fn geometry(ui: &mut Ui, common: crate::Common) -> (f32, f32, f32, f32) {
    (
        ui.get(common.x),
        ui.get(common.y),
        ui.get(common.width),
        ui.get(common.height),
    )
}

#[test]
fn a_row_layout_shares_its_width_as_qt_quick_layouts_do() {
    let mut ui = bare();
    let row = crate::handles::RowLayout::new(&mut ui, None).unwrap();
    let fixed = sized(&mut ui, row.id, 50.0, 20.0);
    let fill = sized(&mut ui, row.id, 0.0, 20.0);
    let capped = sized(&mut ui, row.id, 0.0, 20.0);
    let l = ui.layout_of(fill.id).unwrap();
    l.fill_width.set(&mut ui, true);
    l.fill_height.set(&mut ui, true);
    let l = ui.layout_of(capped.id).unwrap();
    l.fill_width.set(&mut ui, true);
    l.maximum_width.set(&mut ui, 40.0);
    l.alignment.set(&mut ui, crate::align::BOTTOM);
    // Measured: the preferred widths and the spacing; the tallest child.
    assert_eq!(ui.get(row.common.implicit_width), 60.0);
    assert_eq!(ui.get(row.common.implicit_height), 20.0);
    row.common.width.set(&mut ui, 300.0);
    row.common.height.set(&mut ui, 100.0);
    assert_eq!(
        geometry(&mut ui, fixed.common),
        (0.0, 40.0, 50.0, 20.0),
        "centred down"
    );
    assert_eq!(
        geometry(&mut ui, fill.common),
        (55.0, 0.0, 200.0, 100.0),
        "the rest, both ways"
    );
    assert_eq!(
        geometry(&mut ui, capped.common),
        (260.0, 80.0, 40.0, 20.0),
        "its maximum, at the bottom"
    );
    // Short of room, the fillers give it up first, down to their minimum.
    row.common.width.set(&mut ui, 60.0);
    assert_eq!(ui.get(fill.common.width), 0.0);
    assert_eq!(ui.get(fixed.common.width), 50.0);
    // A hidden child takes no room.
    fixed.common.visible.set(&mut ui, false);
    row.common.width.set(&mut ui, 300.0);
    assert_eq!(geometry(&mut ui, fill.common).0, 0.0);
}

#[test]
fn a_column_layout_with_margins_and_preferred_sizes() {
    let mut ui = bare();
    let column = crate::handles::ColumnLayout::new(&mut ui, None).unwrap();
    column.props.spacing.set(&mut ui, 10.0);
    let a = sized(&mut ui, column.id, 30.0, 20.0);
    let b = sized(&mut ui, column.id, 30.0, 20.0);
    let l = ui.layout_of(b.id).unwrap();
    l.preferred_height.set(&mut ui, 50.0);
    l.margins.set(&mut ui, 4.0);
    l.fill_width.set(&mut ui, true);
    assert_eq!(ui.get(column.common.implicit_height), 20.0 + 10.0 + 58.0);
    assert_eq!(ui.get(column.common.implicit_width), 38.0);
    column.common.width.set(&mut ui, 100.0);
    assert_eq!(geometry(&mut ui, a.common), (0.0, 0.0, 30.0, 20.0));
    assert_eq!(geometry(&mut ui, b.common), (4.0, 34.0, 92.0, 50.0));
}

#[test]
fn grids_and_a_flow_place_in_cells_and_wrap() {
    let mut ui = bare();
    let grid = crate::handles::Grid::new(&mut ui, None).unwrap();
    grid.props.columns.set(&mut ui, 2);
    grid.props.spacing.set(&mut ui, 2.0);
    let cells: Vec<_> = [(10.0, 5.0), (20.0, 8.0), (15.0, 4.0)]
        .iter()
        .map(|&(w, h)| sized(&mut ui, grid.id, w, h))
        .collect();
    let places: Vec<(f32, f32)> = cells
        .iter()
        .map(|c| (ui.get(c.common.x), ui.get(c.common.y)))
        .collect();
    assert_eq!(places, [(0.0, 0.0), (17.0, 0.0), (0.0, 10.0)]);
    assert_eq!(ui.get(grid.common.implicit_width), 37.0);
    assert_eq!(ui.get(grid.common.implicit_height), 14.0);

    let layout = crate::handles::GridLayout::new(&mut ui, None).unwrap();
    layout.props.columns.set(&mut ui, 2);
    layout.props.spacing.set(&mut ui, 0.0);
    let label = sized(&mut ui, layout.id, 40.0, 10.0);
    let field = sized(&mut ui, layout.id, 10.0, 10.0);
    ui.layout_of(field.id)
        .unwrap()
        .fill_width
        .set(&mut ui, true);
    layout.common.width.set(&mut ui, 200.0);
    assert_eq!(geometry(&mut ui, label.common).2, 40.0);
    assert_eq!(geometry(&mut ui, field.common), (40.0, 0.0, 160.0, 10.0));

    let flow = crate::handles::Flow::new(&mut ui, None).unwrap();
    flow.props.spacing.set(&mut ui, 5.0);
    let chips: Vec<_> = (0..4)
        .map(|_| sized(&mut ui, flow.id, 30.0, 10.0))
        .collect();
    flow.common.width.set(&mut ui, 70.0);
    let places: Vec<(f32, f32)> = chips
        .iter()
        .map(|c| (ui.get(c.common.x), ui.get(c.common.y)))
        .collect();
    assert_eq!(places, [(0.0, 0.0), (35.0, 0.0), (0.0, 15.0), (35.0, 15.0)]);
    assert_eq!(ui.get(flow.common.implicit_height), 25.0);
}

#[test]
fn a_repeater_makes_a_delegate_per_row_beside_itself() {
    let mut ui = bare();
    let row = crate::handles::Row::new(&mut ui, None).unwrap();
    let first = sized(&mut ui, row.id, 7.0, 5.0);
    let repeater = crate::handles::Repeater::new(&mut ui, Some(row.id)).unwrap();
    let last = sized(&mut ui, row.id, 7.0, 5.0);
    let made = Rc::new(Cell::new(0));
    let counter = Rc::clone(&made);
    ui.set_delegate(
        repeater.id,
        Rc::new(move |ui: &mut Ui, parent| {
            counter.set(counter.get() + 1);
            let r = crate::handles::Rectangle::new(ui, Some(parent))?;
            let (row, index) = ui.delegate_cells(r.id);
            // Each as wide as ten times its row's value.
            r.common.implicit_width.bind(ui, move |ui| {
                let value = ui.read(row);
                let n = match value {
                    crate::Value::Int(n) => n as f32,
                    _ => 0.0,
                };
                10.0 * n
            });
            Some(crate::Delegate {
                root: r.id,
                row,
                index,
            })
        }),
    );
    repeater
        .props
        .model
        .set(&mut ui, crate::ModelSource::list(vec![1_i32, 2, 3]));
    ui.update();
    assert_eq!(ui.get(repeater.props.count), 3);
    assert_eq!(ui.children(row.id).len(), 6);
    assert_eq!(ui.children(row.id)[0], first.id);
    assert_eq!(ui.children(row.id)[1], repeater.id);
    assert_eq!(
        ui.children(row.id)[5],
        last.id,
        "the delegates go beside it"
    );
    assert_eq!(ui.get(last.common.x), 7.0 + 10.0 + 20.0 + 30.0);
    // The same rows again change nothing; other values reuse the
    // delegates; fewer rows drop the last.
    repeater
        .props
        .model
        .set(&mut ui, crate::ModelSource::list(vec![1_i32, 2, 3]));
    ui.update();
    repeater
        .props
        .model
        .set(&mut ui, crate::ModelSource::list(vec![4_i32, 1]));
    ui.update();
    assert_eq!(made.get(), 3);
    assert_eq!(ui.get(last.common.x), 7.0 + 40.0 + 10.0);
    // A program's model: changes are seen at the next update.
    let model = Rc::new(crate::VecModel::new(vec![1_i32]));
    let shared: Rc<dyn crate::Model<Row = i32>> = model.clone();
    repeater.props.model.set(
        &mut ui,
        crate::ModelSource::model(crate::ModelRc::from_rc(shared)),
    );
    ui.update();
    assert_eq!(ui.get(repeater.props.count), 1);
    model.push(5);
    model.set_row(0, 2);
    ui.update();
    assert_eq!(ui.get(repeater.props.count), 2);
    assert_eq!(ui.get(last.common.x), 7.0 + 20.0 + 50.0);
    // Destroying the repeater takes its delegates.
    ui.destroy(repeater.id);
    assert_eq!(ui.children(row.id), [first.id, last.id]);
    assert_eq!(ui.get(last.common.x), 7.0);
}

/// A factory of rectangles `height` high whose `implicitWidth` is their
/// row's value, an `int`.
fn row_factory(height: f32) -> crate::DelegateFactory {
    Rc::new(move |ui: &mut Ui, parent| {
        let r = crate::handles::Rectangle::new(ui, Some(parent))?;
        let (row, index) = ui.delegate_cells(r.id);
        r.common.height.set(ui, height);
        let value: crate::Prop<i32> = crate::Prop::from_id(row);
        r.common
            .implicit_width
            .bind(ui, move |ui| ui.get(value) as f32);
        Some(crate::Delegate {
            root: r.id,
            row,
            index,
        })
    })
}

/// The rows `view` shows, by each shown delegate's value, top to bottom.
fn shown_rows(ui: &mut Ui, view: crate::ItemId) -> Vec<(i32, f32)> {
    let content = ui.content_item(view).unwrap();
    let mut out = Vec::new();
    for child in ui.children(content).to_vec() {
        let c = ui.common(child).unwrap();
        if ui.get(c.visible) {
            out.push((ui.get(c.implicit_width) as i32, ui.get(c.y)));
        }
    }
    out.sort_by(|a, b| a.1.total_cmp(&b.1));
    out
}

#[test]
fn a_list_view_of_a_million_rows_makes_a_screenful() {
    let mut ui = bare();
    let view = crate::handles::ListView::new(&mut ui, None).unwrap();
    view.common.width.set(&mut ui, 200.0);
    view.common.height.set(&mut ui, 100.0);
    ui.set_view_delegates(view.id, vec![row_factory(20.0)], None);
    view.props
        .model
        .set(&mut ui, crate::ModelSource::count(1_000_000));
    ui.update();
    assert_eq!(ui.get(view.props.count), 1_000_000);
    assert_eq!(ui.get(view.props.flick.content_height), 20_000_000.0);
    assert_eq!(
        shown_rows(&mut ui, view.id),
        [(0, 0.0), (1, 20.0), (2, 40.0), (3, 60.0), (4, 80.0)]
    );
    // Scrolled half way: the same five delegates, given other rows.
    view.props.flick.content_y.set(&mut ui, 10_000_010.0);
    ui.update();
    let rows: Vec<i32> = shown_rows(&mut ui, view.id).iter().map(|r| r.0).collect();
    assert_eq!(rows, [500_000, 500_001, 500_002, 500_003, 500_004, 500_005]);
    for _ in 0..50 {
        let y = ui.get(view.props.flick.content_y);
        view.props.flick.content_y.set(&mut ui, y + 7.0);
        ui.update();
    }
    let (held, made) = ui.view_delegates(view.id).unwrap();
    assert!(held <= 7 && made <= 7, "{held} held, {made} made");
    // The wheel scrolls it, and stops at the end.
    ui.position_view_at_end(view.id);
    assert_eq!(ui.get(view.props.flick.content_y), 20_000_000.0 - 100.0);
    assert_eq!(shown_rows(&mut ui, view.id).last().unwrap().0, 999_999);
    ui.pointer_move(50.0, 50.0);
    ui.pointer_wheel(10.0);
    assert_eq!(ui.get(view.props.flick.content_y), 20_000_000.0 - 100.0);
    ui.pointer_wheel(-10.0);
    assert_eq!(ui.get(view.props.flick.content_y), 20_000_000.0 - 130.0);
}

#[test]
fn a_list_view_chooses_delegates_and_follows_its_model() {
    let mut ui = bare();
    let view = crate::handles::ListView::new(&mut ui, None).unwrap();
    view.common.height.set(&mut ui, 100.0);
    view.props.spacing.set(&mut ui, 2.0);
    // Even values are 20 high, odd ones 40.
    let chooser: crate::Chooser = Rc::new(|value: &crate::Value| match value {
        crate::Value::Int(n) => (*n % 2) as usize,
        _ => 0,
    });
    ui.set_view_delegates(
        view.id,
        vec![row_factory(20.0), row_factory(40.0)],
        Some(chooser),
    );
    let model = Rc::new(crate::VecModel::new(vec![0_i32, 1, 2, 3, 4, 5]));
    let shared: Rc<dyn crate::Model<Row = i32>> = model.clone();
    view.props.model.set(
        &mut ui,
        crate::ModelSource::model(crate::ModelRc::from_rc(shared)),
    );
    ui.update();
    assert_eq!(
        shown_rows(&mut ui, view.id),
        [(0, 0.0), (1, 22.0), (2, 64.0), (3, 86.0)]
    );
    // A row that changes kind gets a delegate of its kind.
    model.set_row(0, 7);
    ui.update();
    assert_eq!(shown_rows(&mut ui, view.id)[..2], [(7, 0.0), (1, 42.0)]);
    // Rows added at the end extend it; a reset starts again.
    model.push(8);
    ui.update();
    assert_eq!(ui.get(view.props.count), 7);
    model.set_vec(vec![2, 4]);
    ui.update();
    assert_eq!(shown_rows(&mut ui, view.id), [(2, 0.0), (4, 22.0)]);
    assert_eq!(ui.get(view.props.flick.content_height), 42.0);
}

#[test]
fn a_flickable_scrolls_its_content_by_the_wheel() {
    let mut ui = bare();
    let root = crate::handles::Item::new(&mut ui, None).unwrap();
    root.common.width.set(&mut ui, 100.0);
    root.common.height.set(&mut ui, 100.0);
    let flick = crate::handles::Flickable::new(&mut ui, Some(root.id)).unwrap();
    flick.common.width.set(&mut ui, 100.0);
    flick.common.height.set(&mut ui, 50.0);
    flick.common.clip.set(&mut ui, true);
    let tall = sized(&mut ui, flick.id, 100.0, 80.0);
    let content = ui.content_item(flick.id).unwrap();
    assert_eq!(
        ui.parent(tall.id),
        Some(content),
        "children go on the content"
    );
    let area = area(&mut ui, tall.id, (0.0, 60.0, 100.0, 20.0));
    // The content is as tall as its lowest child.
    let c = ui.common(content).unwrap();
    assert_eq!(ui.get(c.height), 80.0);
    ui.pointer_move(10.0, 10.0);
    ui.pointer_wheel(4.0);
    assert_eq!(ui.get(flick.props.content_y), 12.0);
    ui.pointer_wheel(100.0);
    assert_eq!(
        ui.get(flick.props.content_y),
        30.0,
        "no further than its end"
    );
    // The area at 60 in the content is now at 30 in the window.
    ui.pointer_move(10.0, 35.0);
    ui.pointer_press();
    ui.pointer_release();
    assert!(emitted(&mut ui).contains(&area.props.clicked));
}

#[test]
fn a_behavior_animates_every_change_from_where_it_is() {
    let mut ui = bare();
    let r = crate::handles::Rectangle::new(&mut ui, None).unwrap();
    let wide = ui.prop(false);
    r.common
        .width
        .bind(&mut ui, move |ui| if ui.get(wide) { 200.0 } else { 100.0 });
    ui.behavior(
        r.id,
        r.common.width.id,
        crate::Animate::Always(crate::Tween {
            duration: 100.0,
            easing: crate::Easing::Linear,
        }),
    );
    // Set up before the clock first ticks: shown at once, as a tree being
    // made is, and a control given its value by the code using it.
    wide.set(&mut ui, true);
    ui.update();
    assert!(!ui.animating());
    assert_eq!(ui.get(r.common.width), 200.0);
    wide.set(&mut ui, false);
    ui.update();
    ui.tick(1000.0);
    assert_eq!(ui.get(r.common.width), 100.0, "no animation to begin with");
    // The binding it was given still follows what it read.
    wide.set(&mut ui, true);
    ui.update();
    assert!(ui.animating());
    ui.tick(1050.0);
    assert_eq!(ui.get(r.common.width), 150.0);
    // Set by hand half way: from where it is, to there.
    r.common.width.set(&mut ui, 50.0);
    ui.update();
    ui.tick(1100.0);
    assert_eq!(ui.get(r.common.width), 100.0);
    ui.tick(1150.0);
    assert_eq!(ui.get(r.common.width), 50.0);
    assert!(!ui.animating());
}

#[test]
fn states_choose_values_and_transitions_animate_the_change() {
    let mut ui = bare();
    let r = crate::handles::Rectangle::new(&mut ui, None).unwrap();
    r.common.x.set(&mut ui, 10.0);
    r.props.color.set(&mut ui, Color::hex(0x000000));
    let hot = ui.prop(false);
    let active = ui
        .active_state(
            r.id,
            vec![(
                Rc::from("hot"),
                Rc::new(move |ui: &mut Ui| ui.get(hot)) as _,
            )],
        )
        .unwrap();
    ui.bind_states(
        r.id,
        r.common.x.id,
        active,
        vec![
            (
                Rc::from("moved"),
                Rc::new(|_: &mut Ui| crate::Value::Real(90.0)) as _,
            ),
            (
                Rc::from("hot"),
                Rc::new(|_: &mut Ui| crate::Value::Real(50.0)) as _,
            ),
        ],
    );
    ui.bind_states(
        r.id,
        r.props.color.id,
        active,
        vec![(
            Rc::from("hot"),
            Rc::new(|_: &mut Ui| crate::Value::Color(Color::hex(0xff0000))) as _,
        )],
    );
    ui.behavior(
        r.id,
        r.common.x.id,
        crate::Animate::Transitions {
            active,
            rules: vec![crate::Transition {
                from: Rc::from("*"),
                to: Rc::from("moved"),
                tween: crate::Tween {
                    duration: 200.0,
                    easing: crate::Easing::InOutQuad,
                },
            }],
        },
    );
    assert_eq!(ui.get(r.common.x), 10.0);
    ui.tick(0.0);
    r.common.state.set(&mut ui, Rc::from("moved"));
    ui.update();
    ui.tick(100.0);
    assert_eq!(ui.get(r.common.x), 50.0, "half way along the curve");
    ui.tick(200.0);
    assert_eq!(ui.get(r.common.x), 90.0);
    // `when` wins over `state`; no transition to "hot": it jumps.
    hot.set(&mut ui, true);
    ui.update();
    assert_eq!(ui.get(r.common.x), 50.0);
    assert_eq!(ui.get(r.props.color), Color::hex(0xff0000));
    // Back to its own state: its own value, set while it was elsewhere.
    hot.set(&mut ui, false);
    r.common.state.set(&mut ui, Rc::from(""));
    r.common.x.set(&mut ui, 20.0);
    ui.update();
    assert_eq!(ui.get(r.common.x), 20.0);
    assert_eq!(ui.get(r.props.color), Color::hex(0x000000));
}

#[test]
fn a_sequence_loops_with_pauses() {
    let mut ui = bare();
    let r = crate::handles::Rectangle::new(&mut ui, None).unwrap();
    let running = ui.prop(true);
    let tween = |to: f32| crate::Step::Tween {
        target: r.common.opacity.id,
        from: None,
        to: Some(crate::Value::Real(to)),
        tween: crate::Tween {
            duration: 100.0,
            easing: crate::Easing::Linear,
        },
    };
    let step = crate::Step::Sequential(vec![tween(0.0), crate::Step::Pause(50.0), tween(1.0)]);
    ui.animation_on(r.id, step, -1, running);
    let at = |ui: &mut Ui, t: f64| {
        ui.tick(t);
        ui.get(r.common.opacity)
    };
    assert_eq!(at(&mut ui, 50.0), 0.5);
    assert_eq!(at(&mut ui, 120.0), 0.0, "paused");
    assert_eq!(at(&mut ui, 200.0), 0.5);
    assert_eq!(at(&mut ui, 300.0), 0.5, "round again");
    running.set(&mut ui, false);
    ui.update();
    assert!(!ui.animating());
    assert_eq!(at(&mut ui, 400.0), 0.5, "stopped where it was");
    assert_eq!(crate::Easing::OutCubic.apply(1.0), 1.0);
    assert!((crate::Easing::OutBack.apply(0.7) - 1.0).abs() < 0.2);
}

/// A key as a window hands it over, with Control or Shift held.
fn key_with(key: &str, control: bool, shift: bool) -> crate::KeyEvent {
    let mut event = crate::KeyEvent::named(key);
    event.control = control;
    event.shift = shift;
    if control {
        // What xkb gives for Ctrl+V and the rest: a control character.
        event.text = Rc::from(
            key.chars()
                .next()
                .filter(char::is_ascii_alphabetic)
                .map(|c| ((c.to_ascii_lowercase() as u8 - b'a' + 1) as char).to_string())
                .unwrap_or_default(),
        );
    }
    event
}

/// Type `text` a key at a time.
fn type_text(ui: &mut Ui, text: &str) {
    for c in text.chars() {
        let _ = ui.key_press(&crate::KeyEvent::named(&c.to_string()));
    }
}

/// A focused `TextInput` of Inter 14 in a 300 × 40 root.
fn text_field(ui: &mut Ui) -> crate::handles::TextInput {
    let root = crate::handles::Rectangle::new(ui, None).unwrap();
    root.common.width.set(ui, 300.0);
    root.common.height.set(ui, 40.0);
    let input = crate::handles::TextInput::new(ui, Some(root.id)).unwrap();
    input.common.x.set(ui, 10.0);
    input.common.width.set(ui, 200.0);
    input.common.height.set(ui, 30.0);
    input.props.font.set(ui, inter(14.0));
    ui.force_active_focus(input.id);
    input
}

#[test]
fn a_text_input_types_moves_selects_and_deletes() {
    let mut ui = ui();
    let input = text_field(&mut ui);
    type_text(&mut ui, "hello world");
    assert_eq!(&*ui.get(input.props.text), "hello world");
    assert_eq!(ui.get(input.props.cursor_position), 11);
    // Ctrl+Left goes a word back; Shift+End selects to the end.
    assert!(ui.key_press(&key_with("Left", true, false)));
    assert_eq!(ui.get(input.props.cursor_position), 6);
    assert!(ui.key_press(&key_with("End", false, true)));
    assert_eq!(&*ui.get(input.props.selected_text), "world");
    // Typing replaces the selection; BackSpace takes a character, and
    // Ctrl+BackSpace a word.
    type_text(&mut ui, "tole");
    assert_eq!(&*ui.get(input.props.text), "hello tole");
    assert!(ui.key_press(&crate::KeyEvent::named("BackSpace")));
    assert_eq!(&*ui.get(input.props.text), "hello tol");
    assert!(ui.key_press(&key_with("BackSpace", true, false)));
    assert_eq!(&*ui.get(input.props.text), "hello ");
    // Home, then Delete.
    assert!(ui.key_press(&crate::KeyEvent::named("Home")));
    assert!(ui.key_press(&crate::KeyEvent::named("Delete")));
    assert_eq!(&*ui.get(input.props.text), "ello ");
    // Return says `accepted` and types nothing; Tab is not taken.
    let _ = ui.take_signals();
    assert!(ui.key_press(&crate::KeyEvent::named("Return")));
    assert_eq!(&*ui.get(input.props.text), "ello ");
    assert!(
        ui.take_signals()
            .iter()
            .any(|e| e.signal == input.props.accepted)
    );
    assert!(!ui.key_press(&crate::KeyEvent::named("Tab")));
    // Characters wider than a byte stay whole.
    assert!(ui.key_press(&crate::KeyEvent::named("End")));
    type_text(&mut ui, "äö");
    assert!(ui.key_press(&crate::KeyEvent::named("BackSpace")));
    assert_eq!(&*ui.get(input.props.text), "ello ä");
}

#[test]
fn control_never_types_and_the_clipboard_copies_cuts_and_pastes() {
    let mut ui = ui();
    let input = text_field(&mut ui);
    type_text(&mut ui, "key.pem");
    // Ctrl+V with nothing copied types no `v` (the viewer's bug).
    assert!(ui.key_press(&key_with("v", true, false)));
    assert_eq!(&*ui.get(input.props.text), "key.pem");
    // Alt+letters type nothing either.
    let mut alt = crate::KeyEvent::named("x");
    alt.alt = true;
    assert!(!ui.key_press(&alt));
    assert_eq!(&*ui.get(input.props.text), "key.pem");
    // Ctrl+A, Ctrl+C: kept, and handed to a window.
    assert!(ui.key_press(&key_with("a", true, false)));
    assert!(ui.key_press(&key_with("c", true, false)));
    assert_eq!(ui.take_copied().as_deref(), Some("key.pem"));
    assert_eq!(ui.take_copied(), None);
    // Ctrl+X empties it; Ctrl+V twice puts it back twice.
    assert!(ui.key_press(&key_with("x", true, false)));
    assert_eq!(&*ui.get(input.props.text), "");
    assert!(ui.key_press(&key_with("v", true, false)));
    assert!(ui.key_press(&key_with("V", true, false)));
    assert_eq!(&*ui.get(input.props.text), "key.pemkey.pem");
    // With a window's clipboard, a paste waits for the window.
    ui.set_clipboard_external(true);
    assert!(ui.key_press(&key_with("a", true, false)));
    assert!(ui.key_press(&key_with("v", true, false)));
    assert_eq!(&*ui.get(input.props.text), "key.pemkey.pem");
    assert!(ui.take_paste_request());
    ui.paste("\n~/.ssh/id_ed25519\nsecond\n");
    assert_eq!(
        &*ui.get(input.props.text),
        "~/.ssh/id_ed25519",
        "one line: the first that is not blank"
    );
    assert!(!ui.take_paste_request());
    // A password is not copied.
    let _ = ui.take_copied();
    input.props.echo_mode.set(&mut ui, crate::echo::PASSWORD);
    assert!(ui.key_press(&key_with("a", true, false)));
    assert!(ui.key_press(&key_with("c", true, false)));
    assert_eq!(ui.take_copied(), None);
}

#[test]
fn the_pointer_places_the_caret_drags_a_selection_and_double_clicks_a_word() {
    let mut ui = ui();
    let input = text_field(&mut ui);
    input.props.text.set(&mut ui, Rc::from("alpha beta gamma"));
    ui.set_focus(input.id, false);
    let rect = ui.rect_of(input.id).unwrap();
    let mid = rect.y + rect.h / 2.0;
    // Pressed far left: the caret at the start, and the focus taken.
    ui.pointer_move(rect.x + 1.0, mid);
    ui.pointer_press();
    ui.pointer_release();
    assert!(ui.get(input.common.active_focus));
    assert_eq!(ui.get(input.props.cursor_position), 0);
    // Dragged across to the end: all selected.
    ui.tick(1000.0);
    ui.pointer_press();
    ui.pointer_move(rect.x + 199.0, mid);
    ui.pointer_release();
    assert_eq!(&*ui.get(input.props.selected_text), "alpha beta gamma");
    // A double click on "beta" takes the word.
    ui.tick(2000.0);
    let beta = rect.x + ui.text_width(&inter(14.0), "alpha be");
    ui.pointer_move(beta, mid);
    ui.pointer_press();
    ui.pointer_release();
    ui.pointer_press();
    ui.pointer_release();
    assert_eq!(&*ui.get(input.props.selected_text), "beta");
    // The I-beam over it.
    assert_eq!(ui.take_cursor(), Some(crate::Cursor::Text));
    // It draws its caret and selection only while focused.
    let focused = ui.scene();
    ui.set_window_active(false);
    assert_ne!(focused, ui.scene());
}

#[test]
fn a_text_edit_breaks_lines_and_moves_between_them() {
    let mut ui = ui();
    let root = crate::handles::Item::new(&mut ui, None).unwrap();
    let edit = crate::handles::TextEdit::new(&mut ui, Some(root.id)).unwrap();
    edit.props.font.set(&mut ui, inter(14.0));
    edit.common.width.set(&mut ui, 120.0);
    ui.force_active_focus(edit.id);
    type_text(&mut ui, "one");
    assert!(ui.key_press(&crate::KeyEvent::named("Return")));
    type_text(&mut ui, "two");
    assert_eq!(&*ui.get(edit.props.text), "one\ntwo");
    let one_line = ui.get(edit.common.implicit_height) / 2.0;
    // Up goes to the line above, at the same place across.
    assert!(ui.key_press(&crate::KeyEvent::named("Up")));
    assert_eq!(ui.get(edit.props.cursor_position), 3);
    assert!(ui.key_press(&crate::KeyEvent::named("Down")));
    assert_eq!(ui.get(edit.props.cursor_position), 7);
    // Long text breaks between words when wrapping is on.
    edit.props.text.set(
        &mut ui,
        Rc::from("the quick brown fox jumps over the lazy dog"),
    );
    let unwrapped = ui.get(edit.common.implicit_height);
    assert!((unwrapped - one_line).abs() < 0.01);
    edit.props.wrap_mode.set(&mut ui, crate::wrap::WRAP);
    let wrapped = ui.get(edit.common.implicit_height);
    assert!(
        wrapped >= 3.0 * one_line,
        "{wrapped} for lines of {one_line}"
    );
    for op in ui.scene() {
        if let DrawOp::Text { run, .. } = op {
            assert!(
                run.width / ui.scale() <= 120.5,
                "a line is wider than the edit"
            );
        }
    }
}

#[test]
fn a_canvas_paints_through_its_rust_painter_and_again_when_told() {
    let mut ui = bare();
    let canvas = crate::handles::Canvas::new(&mut ui, None).unwrap();
    canvas.common.x.set(&mut ui, 10.0);
    canvas.common.width.set(&mut ui, 100.0);
    canvas.common.height.set(&mut ui, 40.0);
    let height = Rc::new(Cell::new(10.0_f32));
    let data = Rc::clone(&height);
    canvas.props.painter.set(
        &mut ui,
        crate::CanvasPainter::new(move |p| {
            let (w, h) = p.size();
            p.fill_rect(
                crate::Rect::new(0.0, h - data.get(), w, data.get()),
                0.0,
                Color::hex(0x2a62c9),
            );
        }),
    );
    let first = ui.scene();
    assert_eq!(
        first,
        [DrawOp::Fill {
            rect: crate::Rect::new(10.0, 30.0, 100.0, 10.0),
            radius: 0.0,
            color: Color::hex(0x2a62c9)
        }],
        "in the canvas's own coordinates, placed"
    );
    let _ = ui.take_changed();
    height.set(20.0);
    ui.update_canvas(canvas.id);
    assert!(ui.take_changed(), "a frame is wanted");
    assert_ne!(ui.scene(), first);
}

#[test]
fn a_path_is_bounded_and_drawn() {
    let mut ui = bare();
    let shape = crate::handles::Shape::new(&mut ui, None).unwrap();
    let mut path = crate::Path::new();
    let _ = path
        .move_to(0.0, 0.0)
        .line_to(18.0, 10.0)
        .line_to(0.0, 20.0)
        .close();
    assert_eq!(path.bounds(), Some(crate::Rect::new(0.0, 0.0, 18.0, 20.0)));
    shape.props.paths.set(
        &mut ui,
        crate::ShapePaths::new(vec![crate::ShapePathValue {
            path,
            fill: Color::hex(0xff0000),
            stroke: Color::TRANSPARENT,
            width: 0.0,
        }]),
    );
    assert_eq!(
        ui.get(shape.common.width),
        18.0,
        "its implicit size is its paths'"
    );
    assert_eq!(ui.get(shape.common.height), 20.0);
    shape.common.x.set(&mut ui, 5.0);
    let frame = ui.render((30, 30), Color::TRANSPARENT).unwrap();
    let at = |x: u32, y: u32| frame.pixel(x, y).unwrap().red();
    assert_eq!(at(8, 10), 255, "inside the triangle");
    assert_eq!(at(20, 10), 255, "near its tip");
    assert_eq!(at(20, 3), 0, "outside");
}

#[test]
fn a_painter_draws_discs_flat_ended_and_dashed_lines_and_cut_text() {
    let mut ui = ui();
    let canvas = crate::handles::Canvas::new(&mut ui, None).unwrap();
    canvas.common.width.set(&mut ui, 60.0);
    canvas.common.height.set(&mut ui, 40.0);
    let ink = Color::hex(0x1a1a1a);
    canvas.props.painter.set(
        &mut ui,
        crate::CanvasPainter::new(move |p| {
            p.circle((10.0, 10.0), 4.0, ink);
            // Flat ends stop at the points; a dashed line has gaps.
            p.polyline(
                &[(20.0, 10.0), (40.0, 10.0)],
                4.0,
                ink,
                crate::LineStyle {
                    cap: crate::LineCap::Butt,
                    miter: false,
                    dash: 0.0,
                },
            );
            p.polyline(
                &[(0.0, 30.0), (60.0, 30.0)],
                2.0,
                ink,
                crate::LineStyle {
                    cap: crate::LineCap::Butt,
                    miter: false,
                    dash: 4.0,
                },
            );
            p.push_clip(crate::Rect::new(0.0, 0.0, 60.0, 20.0));
            let drawn = p.text_in(
                "a long label",
                &inter(12.0),
                (0.0, 0.0, 20.0),
                ink,
                Some(20.0),
            );
            assert!(drawn <= 20.0, "cut to its room: {drawn}");
            p.pop_clip();
        }),
    );
    let frame = ui.render((60, 40), Color::hex(0xffffff)).unwrap();
    let dark = |x: u32, y: u32| pixel(&frame, x, y)[0] < 128;
    assert!(dark(10, 10), "the disc's middle");
    assert!(!dark(10, 15), "past its radius");
    assert!(dark(21, 10) && dark(39, 10));
    assert!(!dark(41, 10), "a flat end stops at its point");
    let line: Vec<bool> = (0..60).map(|x| dark(x, 30)).collect();
    assert!(
        line.iter().any(|d| *d) && line.iter().any(|d| !*d),
        "dashes and gaps"
    );
}

#[test]
fn a_dashed_border_has_gaps() {
    let mut ui = ui();
    let r = crate::handles::Rectangle::new(&mut ui, None).unwrap();
    r.common.width.set(&mut ui, 40.0);
    r.common.height.set(&mut ui, 20.0);
    r.props.border_width.set(&mut ui, 1.0);
    r.props.border_color.set(&mut ui, Color::hex(0x000000));
    r.props.border_dash.set(&mut ui, 3.0);
    let frame = ui.render((40, 20), Color::hex(0xffffff)).unwrap();
    let top: Vec<bool> = (4..36).map(|x| pixel(&frame, x, 0)[0] < 160).collect();
    assert!(top.iter().any(|d| *d) && top.iter().any(|d| !*d), "{top:?}");
}

#[test]
fn a_list_scrolled_before_it_is_laid_out_makes_only_its_screenful() {
    // A model that says its heights, scrolled half way before the first
    // update: the view starts at the row there, not at the top.
    let mut ui = bare();
    let view = crate::handles::ListView::new(&mut ui, None).unwrap();
    view.common.width.set(&mut ui, 200.0);
    view.common.height.set(&mut ui, 100.0);
    ui.set_view_delegates(view.id, vec![row_factory(20.0)], None);
    struct Rows(crate::ModelTracker);
    impl crate::Model for Rows {
        type Row = i32;
        fn row_count(&self) -> usize {
            100_000
        }
        fn row(&self, index: usize) -> Option<i32> {
            i32::try_from(index).ok()
        }
        fn row_height(&self, _index: usize) -> Option<f32> {
            Some(20.0)
        }
        fn tracker(&self) -> &crate::ModelTracker {
            &self.0
        }
    }
    view.props.model.set(
        &mut ui,
        crate::ModelSource::model(crate::ModelRc::new(Rows(crate::ModelTracker::default()))),
    );
    view.props.flick.content_y.set(&mut ui, 1_000_000.0);
    ui.update();
    let (_, made) = ui.view_delegates(view.id).unwrap();
    assert!(made <= 8, "{made} made");
    assert_eq!(shown_rows(&mut ui, view.id).first().unwrap().0, 50_000);
}
