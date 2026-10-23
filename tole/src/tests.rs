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
    assert_eq!(got, [a.props.on_pressed, a.props.released]);
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
    a.props.enabled.set(&mut ui, false);
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
