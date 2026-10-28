//! A window: a tree shown in a Wayland toplevel, or a layer-shell
//! surface (a bar, a dock, a panel at a screen's edge), through
//! `compositor-toolkit` (`docs/UI-FRAMEWORK.md` §4.10), on Ferrix under
//! hyprix and on any Wayland desktop -- the toolkit is pure Rust over the
//! socket.
//!
//! [`Window::dispatch`] waits for what happens, feeds the pointer to the
//! `Ui`, sizes the root to the window, and draws what changed: only the
//! damaged parts are drawn again and handed over, paced by the
//! compositor's frame callbacks. What tole does not take -- keys, timers,
//! a close request -- comes back as the toolkit's events.
//!
//! The clipboard goes through `ext_data_control_v1` when the compositor
//! offers it (hyprix does): a copy in a text field is offered as the
//! selection, and a paste reads the selection's text, or takes the
//! window's own copy while that is still the selection. Without it the
//! `Ui` keeps copies to itself.

use std::time::Duration;

pub use compositor_toolkit::{Anchor, KeyboardInteractivity, Layer, LayerOptions, Margin};
use compositor_toolkit::{
    Client, CursorShape, Error, Event, KeyboardEvent, ObjectId, PointerEvent, SurfaceId,
    ToplevelOptions, Value,
};

use crate::Frame;
use crate::item::Common;
use crate::ui::Ui;
use crate::value::Color;

/// The left button's evdev code.
const BUTTON_LEFT: u32 = 0x110;

/// What a window is opened with.
#[derive(Clone, Debug)]
pub struct WindowOptions {
    /// Its title.
    pub title: String,
    /// Its `app_id`: the `.desktop` file's name without its suffix.
    pub app_id: String,
    /// The size asked for, in logical pixels, while the compositor lets
    /// the window choose.
    pub size: (u32, u32),
    /// What shows where nothing is drawn.
    pub background: Color,
    /// The smallest size it may be made; (0, 0) for any.
    pub min_size: (u32, u32),
    /// Whether the window resizes itself from its edges, for a program
    /// that draws its own frame where the compositor draws none (GNOME's
    /// mutter): a press within a few pixels of an edge drags it.
    pub resize_edges: bool,
    /// Whether the window binds the desktop's clipboard itself
    /// (`ext_data_control_v1`). A program that keeps its own selections
    /// says false, and then serves the `Ui`'s copies and pastes itself
    /// (`Ui::take_copied`, `Ui::take_paste_request`, `Ui::paste`).
    pub clipboard: bool,
    /// A layer-shell surface placed as this says -- a bar, a dock, a
    /// panel held against a screen's edge (`zwlr_layer_shell_v1`) --
    /// rather than a toplevel; `title` and `min_size` are then unused, and
    /// `size` is what the compositor fills in where the options say 0.
    pub layer: Option<LayerOptions>,
    /// Who the program would have draw its frame -- the title bar and its
    /// minimize, maximize and close -- asked of the compositor through
    /// `xdg-decoration`. The compositor decides; [`Window::draws_own_frame`]
    /// says what it decided, and the toolkit's
    /// [`compositor_toolkit::Event::Decorated`] when that changes.
    pub decorations: Decorations,
}

/// Who a window asks to draw its frame ([`WindowOptions::decorations`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Decorations {
    /// The compositor: its title bar over the window, which draws none of
    /// its own. What every window asks unless it says otherwise.
    #[default]
    Server,
    /// The program itself: its own header with its own buttons, which the
    /// compositor leaves alone where it agrees (`client_side`) -- and
    /// which the program shows only then, or where the compositor has no
    /// decoration manager at all.
    Client,
}

impl Default for WindowOptions {
    fn default() -> Self {
        WindowOptions {
            title: String::new(),
            app_id: String::new(),
            size: (800, 600),
            background: Color::hex(0xff_ff_ff),
            min_size: (0, 0),
            resize_edges: false,
            clipboard: true,
            layer: None,
            decorations: Decorations::Server,
        }
    }
}

/// How close to an edge a press resizes the window, in logical pixels.
const EDGE: f32 = 6.0;

/// The `xdg_toplevel.resize_edge` at (`x`, `y`) in a window of `size`, 0
/// for none, and the pointer's shape there (cursor-shape-v1's numbers).
fn edge_at(x: f32, y: f32, size: (u32, u32)) -> (u32, CursorShape) {
    let (w, h) = (size.0 as f32, size.1 as f32);
    let top = y < EDGE;
    let bottom = y >= h - EDGE;
    let left = x < EDGE;
    let right = x >= w - EDGE;
    match (top, bottom, left, right) {
        (true, _, true, _) => (5, CursorShape::Other(29)),
        (true, _, _, true) => (9, CursorShape::Other(28)),
        (_, true, true, _) => (6, CursorShape::Other(28)),
        (_, true, _, true) => (10, CursorShape::Other(29)),
        (true, ..) => (1, CursorShape::Other(27)),
        (_, true, ..) => (2, CursorShape::Other(27)),
        (_, _, true, _) => (4, CursorShape::Other(26)),
        (_, _, _, true) => (8, CursorShape::Other(26)),
        _ => (0, CursorShape::Default),
    }
}

/// How a window's frames went, for a program that measures itself.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameStats {
    /// Frames handed to the compositor.
    pub frames: u64,
    /// How long the last one took to draw and hand over.
    pub last: Duration,
    /// How long all of them took.
    pub total: Duration,
    /// When the last one was handed over.
    pub at: Option<std::time::Instant>,
}

/// A tree in a toplevel.
#[derive(Debug)]
pub struct Window {
    /// The connection.
    client: Client,
    /// The toplevel.
    surface: SurfaceId,
    /// The root item's properties: its size follows the window's.
    root: Common,
    /// The frame kept between draws.
    frame: Option<Frame>,
    /// The options it was opened with.
    options: WindowOptions,
    /// Its size in logical pixels, once configured.
    size: Option<(u32, u32)>,
    /// The compositor's scale for it.
    scale: u32,
    /// A frame was handed over and its callback has not come.
    awaiting: bool,
    /// Something changed while a frame was awaited.
    pending: bool,
    /// The compositor asked it to close, or closed it.
    closed: bool,
    /// When it opened: the animations' clock counts from here.
    opened: std::time::Instant,
    /// The desktop's clipboard, when the compositor offers one.
    clipboard: Option<Clipboard>,
    /// The edge the pointer is over, as `xdg_toplevel.resize_edge`; 0
    /// none.
    edge: u32,
    /// The last key press's serial, which a program's own copy names.
    key_serial: u32,
    /// How the frames went.
    stats: FrameStats,
    /// The GPU's frames, when it draws them.
    #[cfg(feature = "gpu")]
    presenter: Option<crate::gpu::present::Presenter>,
    /// The GPU being made, and what the compositor said of dmabufs: the
    /// window draws on the CPU until it is ready.
    #[cfg(feature = "gpu")]
    starting: Option<(crate::gpu::Pending, crate::gpu::present::Feedback)>,
    /// Events read before the program asked, given with the next dispatch.
    deferred: Vec<Event>,
}

/// The types text is offered and read as, best first.
const TEXT_TYPES: [&str; 3] = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"];

/// The desktop's clipboard through `ext_data_control_v1`.
#[derive(Debug)]
struct Clipboard {
    /// The manager.
    manager: ObjectId,
    /// The seat's data device.
    device: ObjectId,
    /// The source this window offers, and its text, until another
    /// program's copy cancels it.
    offered: Option<(ObjectId, std::rc::Rc<str>)>,
    /// The offer the selection is now, and the types it said it has.
    selection: Option<ObjectId>,
    /// Every live offer's types.
    types: Vec<(ObjectId, String)>,
}

impl Clipboard {
    /// The clipboard, when the compositor has `ext_data_control`.
    fn bind(client: &mut Client) -> Option<Self> {
        use compositor_toolkit::protocol::ext_data_control::{
            self as control, ext_data_control_device_v1 as device,
            ext_data_control_manager_v1 as manager,
        };
        let seat = client.seat()?;
        let (bound, _) = client
            .bind(&control::EXT_DATA_CONTROL_MANAGER_V1, 1, None)
            .ok()?;
        let made = client.new_object(&control::EXT_DATA_CONTROL_DEVICE_V1, 1);
        client
            .request(
                bound,
                manager::request::GET_DATA_DEVICE,
                &[Value::NewId(made), Value::Object(seat)],
            )
            .ok()?;
        client.adopt_new_ids(
            made,
            device::event::DATA_OFFER,
            &control::EXT_DATA_CONTROL_OFFER_V1,
            1,
        );
        Some(Clipboard {
            manager: bound,
            device: made,
            offered: None,
            selection: None,
            types: Vec::new(),
        })
    }

    /// Offer `text` as the selection.
    fn copy(&mut self, client: &mut Client, text: std::rc::Rc<str>) {
        use compositor_toolkit::protocol::ext_data_control::{
            self as control, ext_data_control_device_v1 as device,
            ext_data_control_manager_v1 as manager, ext_data_control_source_v1 as source,
        };
        let made = client.new_object(&control::EXT_DATA_CONTROL_SOURCE_V1, 1);
        if client
            .request(
                self.manager,
                manager::request::CREATE_DATA_SOURCE,
                &[Value::NewId(made)],
            )
            .is_err()
        {
            return;
        }
        for mime in TEXT_TYPES {
            let _ = client.request(
                made,
                source::request::OFFER,
                &[Value::Str(Some(mime.to_owned()))],
            );
        }
        let _ = client.request(
            self.device,
            device::request::SET_SELECTION,
            &[Value::Object(made)],
        );
        let _ = client.flush();
        if let Some((old, _)) = self.offered.replace((made, text)) {
            let _ = client.request(old, source::request::DESTROY, &[]);
        }
    }

    /// An event for one of the clipboard's objects; whether it was one.
    fn event(
        &mut self,
        client: &mut Client,
        object: ObjectId,
        opcode: u16,
        args: &[Value],
    ) -> bool {
        use compositor_toolkit::protocol::ext_data_control::{
            ext_data_control_device_v1 as device, ext_data_control_offer_v1 as offer,
            ext_data_control_source_v1 as source,
        };
        if let Some((offered, text)) = &self.offered
            && object == *offered
        {
            if opcode == source::event::SEND {
                if let Some(Value::Fd(fd)) = args.get(1) {
                    write_and_close(*fd, text.as_bytes());
                }
            } else if opcode == source::event::CANCELLED {
                let _ = client.request(*offered, source::request::DESTROY, &[]);
                self.offered = None;
            }
            return true;
        }
        if object == self.device {
            if opcode == device::event::SELECTION {
                let named = args
                    .first()
                    .and_then(Value::as_object)
                    .filter(|o| !o.is_null());
                if let Some(old) = self.selection.take()
                    && Some(old) != named
                {
                    let _ = client.request(old, offer::request::DESTROY, &[]);
                    self.types.retain(|(o, _)| *o != old);
                }
                self.selection = named;
            }
            return true;
        }
        if opcode == offer::event::OFFER
            && let Some(mime) = args.first().and_then(Value::as_str)
        {
            self.types.push((object, mime.to_owned()));
            return true;
        }
        false
    }

    /// The selection's text: the window's own copy while it is the
    /// selection, else read from the program that copied it, waiting a
    /// moment for it.
    fn read(&mut self, client: &mut Client) -> String {
        use compositor_toolkit::protocol::ext_data_control::ext_data_control_offer_v1 as offer;
        use std::io::Read as _;
        use std::os::fd::AsRawFd as _;
        if let Some((_, text)) = &self.offered {
            return text.to_string();
        }
        let Some(held) = self.selection else {
            return String::new();
        };
        let Some(mime) = TEXT_TYPES.iter().find(|wanted| {
            self.types
                .iter()
                .any(|(o, m)| *o == held && m.as_str() == **wanted)
        }) else {
            return String::new();
        };
        let Ok((mut reading, writing)) = std::os::unix::net::UnixStream::pair() else {
            return String::new();
        };
        let sent = client.request(
            held,
            offer::request::RECEIVE,
            &[
                Value::Str(Some((*mime).to_owned())),
                Value::Fd(writing.as_raw_fd()),
            ],
        );
        let _ = client.flush();
        drop(writing);
        if sent.is_err() {
            return String::new();
        }
        let _ = reading.set_read_timeout(Some(Duration::from_millis(500)));
        let mut bytes = Vec::new();
        let _ = reading.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// Write `bytes` to the descriptor a paste handed over, and close it.
fn write_and_close(fd: i32, bytes: &[u8]) {
    use std::io::Write as _;
    use std::os::fd::FromRawFd as _;
    // SAFETY: the descriptor arrived with the compositor's `send` event and
    // nothing else in this process holds it; the file owns and closes it.
    let mut pipe = unsafe { std::fs::File::from_raw_fd(fd) };
    let _ = pipe.write_all(bytes);
}

impl Window {
    /// Connect to `$WAYLAND_DISPLAY` and open a toplevel showing the tree
    /// whose root is `root`.
    ///
    /// # Errors
    ///
    /// No compositor, or one without `xdg_wm_base`.
    pub fn open(root: Common, options: WindowOptions) -> Result<Self, Error> {
        #[cfg(feature = "gpu")]
        return Self::open_with(root, options, crate::gpu::Choice::FromEnv);
        #[cfg(not(feature = "gpu"))]
        Self::opened(root, options)
    }

    /// [`Window::open`], its frames drawn by the renderer `choice` names:
    /// the GPU falls back to the CPU, said on standard error, when there
    /// is no Vulkan device or it fails. [`Window::open`] takes what
    /// `TOLE_RENDERER` says, and the CPU when it says nothing.
    ///
    /// # Errors
    ///
    /// As [`Window::open`].
    #[cfg(feature = "gpu")]
    pub fn open_with(
        root: Common,
        options: WindowOptions,
        choice: crate::gpu::Choice,
    ) -> Result<Self, Error> {
        let mut window = Self::opened(root, options)?;
        if let Some((pending, said, events)) = start_gpu(choice, &mut window.client) {
            window.starting = Some((pending, said));
            window.deferred = events;
        }
        Ok(window)
    }

    /// The window, drawn on the CPU.
    fn opened(root: Common, options: WindowOptions) -> Result<Self, Error> {
        let mut client = Client::connect()?;
        let surface = match &options.layer {
            Some(layer) => client.layer_surface(layer)?,
            None => client.toplevel(&ToplevelOptions {
                title: options.title.clone(),
                app_id: options.app_id.clone(),
                size: options.size,
                min_size: options.min_size,
                own_frame: Some(options.decorations == Decorations::Client),
                ..ToplevelOptions::default()
            })?,
        };
        let clipboard = if options.clipboard {
            Clipboard::bind(&mut client)
        } else {
            None
        };
        Ok(Window {
            #[cfg(feature = "gpu")]
            presenter: None,
            #[cfg(feature = "gpu")]
            starting: None,
            deferred: Vec::new(),
            clipboard,
            client,
            surface,
            root,
            frame: None,
            options,
            size: None,
            scale: 1,
            awaiting: false,
            pending: true,
            closed: false,
            opened: std::time::Instant::now(),
            edge: 0,
            key_serial: 0,
            stats: FrameStats::default(),
        })
    }

    /// Move the window by the pointer, from the press that asked: what a
    /// press on a program's own title bar does.
    pub fn start_move(&mut self) {
        self.client.drag_window(self.surface, 0);
    }

    /// Maximize it, or give it back its own size: from what the
    /// compositor last said it is, so a window maximized by a key or by the
    /// compositor's own bar is restored by the program's button.
    pub fn toggle_maximized(&mut self) {
        let state = if !self.client.is_maximized(self.surface) {
            compositor_toolkit::WindowState::Maximized
        } else {
            compositor_toolkit::WindowState::Restored
        };
        self.client.set_window_state(self.surface, state);
    }

    /// Place a layer surface anew: its size, edges, zone, margins or
    /// keyboard (`output` and `namespace` stay as opened). Nothing for a
    /// toplevel.
    pub fn set_layer(&mut self, layer: LayerOptions) {
        if self.options.layer.is_some() {
            self.client.set_layer_options(self.surface, &layer);
            self.options.layer = Some(layer);
        }
    }

    /// Where the pointer reaches the window, in logical pixels; `None`
    /// for everywhere. A layer surface wider than what it draws -- a
    /// popover's room above a dock -- lets the pointer through elsewhere.
    pub fn set_input_region(&mut self, region: Option<&[compositor_toolkit::Rect]>) {
        self.client.set_input_region(self.surface, region);
    }

    /// Whether the program draws its own frame -- its title bar, the
    /// window's minimize, maximize and close, and moving the window by its
    /// header (`xdg_toplevel.move`): true where the compositor answered
    /// `client_side`, or has no decoration manager, so no frame but the
    /// program's would be drawn. False until the compositor's first answer,
    /// and wherever it draws the frame itself. A program shows its own
    /// buttons only while this is true.
    #[must_use]
    pub fn draws_own_frame(&self) -> bool {
        self.options.layer.is_none() && self.client.own_frame(self.surface) == Some(true)
    }

    /// Whether the compositor says the window is maximized.
    #[must_use]
    pub fn is_maximized(&self) -> bool {
        self.client.is_maximized(self.surface)
    }

    /// Minimize it.
    pub fn minimize(&mut self) {
        self.client
            .set_window_state(self.surface, compositor_toolkit::WindowState::Minimized);
    }

    /// The last key press's serial: what a program that keeps its own
    /// selections names a copy with.
    #[must_use]
    pub fn key_serial(&self) -> u32 {
        self.key_serial
    }

    /// How its frames went so far.
    #[must_use]
    pub fn frame_stats(&self) -> FrameStats {
        self.stats
    }

    /// What draws its frames, in words: `cpu`, or `gpu:` with the
    /// device's name and how the frames reach the compositor.
    #[must_use]
    pub fn renderer(&self) -> String {
        #[cfg(feature = "gpu")]
        if let Some(presenter) = &self.presenter {
            return presenter.describe();
        }
        "cpu".to_owned()
    }

    /// Its size in logical pixels, once the compositor gave one.
    #[must_use]
    pub fn size(&self) -> Option<(u32, u32)> {
        self.size
    }

    /// Draw what the program changed in `ui` since the last dispatch,
    /// without waiting for anything to happen first.
    ///
    /// # Errors
    ///
    /// The connection failed.
    pub fn refresh(&mut self, ui: &mut Ui) -> Result<(), Error> {
        ui.update();
        if ui.take_changed() {
            self.pending = true;
        }
        if self.pending && !self.awaiting {
            self.draw(ui)?;
        }
        Ok(())
    }

    /// The connection, for what the program does with it itself: timers,
    /// the clipboard, a cursor.
    pub fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    /// The toplevel.
    #[must_use]
    pub fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Whether the compositor asked it to close.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Wait up to `timeout` (for ever when `None`) for something to
    /// happen, hand the pointer to `ui`, and draw what changed. Answers
    /// the events tole did not take.
    ///
    /// # Errors
    ///
    /// The connection failed.
    pub fn dispatch(
        &mut self,
        ui: &mut Ui,
        timeout: Option<Duration>,
    ) -> Result<Vec<Event>, Error> {
        // While anything animates, wake at least once a frame.
        let timeout = if ui.animating() {
            let frame = Duration::from_millis(8);
            Some(timeout.map_or(frame, |t| t.min(frame)))
        } else {
            timeout
        };
        // While the device draws a frame, often enough to commit it as
        // soon as it is drawn.
        #[cfg(feature = "gpu")]
        let timeout = if self
            .presenter
            .as_ref()
            .is_some_and(crate::gpu::present::Presenter::in_flight)
        {
            let soon = Duration::from_millis(1);
            Some(timeout.map_or(soon, |t| t.min(soon)))
        } else {
            timeout
        };
        // And when the focused field's caret turns.
        let timeout = match ui.next_blink() {
            Some(ms) => {
                let blink = Duration::from_millis(ms.ceil().max(1.0) as u64);
                Some(timeout.map_or(blink, |t| t.min(blink)))
            }
            None => timeout,
        };
        if self.options.clipboard {
            ui.set_clipboard_external(self.clipboard.is_some());
        }
        #[cfg(feature = "gpu")]
        let mut events = self
            .presenter
            .as_mut()
            .map(crate::gpu::present::Presenter::take_deferred)
            .unwrap_or_default();
        #[cfg(not(feature = "gpu"))]
        let mut events = Vec::new();
        events.append(&mut self.deferred);
        events.extend(self.client.dispatch(timeout)?);
        #[cfg(feature = "gpu")]
        self.adopt_gpu();
        ui.tick(self.opened.elapsed().as_secs_f64() * 1000.0);
        let mut rest = Vec::new();
        for event in events {
            match event {
                Event::Configure {
                    surface,
                    width,
                    height,
                } if surface == self.surface => {
                    let (w, h) = if width == 0 || height == 0 {
                        self.options.size
                    } else {
                        (width, height)
                    };
                    self.size = Some((w, h));
                    self.root.width.set(ui, w as f32);
                    self.root.height.set(ui, h as f32);
                    self.pending = true;
                }
                Event::Scale { surface, scale } if surface == self.surface => {
                    self.scale = scale.max(1);
                    ui.set_scale(self.scale as f32);
                    self.pending = true;
                }
                Event::Frame { surface, .. } if surface == self.surface => {
                    self.awaiting = false;
                }
                Event::CloseRequested(surface) | Event::Closed(surface)
                    if surface == self.surface =>
                {
                    self.closed = true;
                    rest.push(event);
                }
                Event::Pointer(pointer) => self.pointer(ui, pointer, &mut rest),
                Event::Keyboard(keyboard) => self.keyboard(ui, keyboard, &mut rest),
                #[cfg(feature = "gpu")]
                Event::Object { object, opcode, .. }
                    if self
                        .presenter
                        .as_mut()
                        .is_some_and(|p| p.event(object, opcode)) => {}
                Event::Object {
                    object,
                    opcode,
                    ref args,
                    ..
                } if self
                    .clipboard
                    .as_mut()
                    .is_some_and(|c| c.event(&mut self.client, object, opcode, args)) => {}
                other => rest.push(other),
            }
        }
        #[cfg(feature = "gpu")]
        self.settle_gpu();
        // What the text fields asked of the clipboard, unless the
        // program serves it itself.
        if self.options.clipboard
            && let Some(text) = ui.take_copied()
            && let Some(clipboard) = self.clipboard.as_mut()
        {
            clipboard.copy(&mut self.client, text);
        }
        if self.options.clipboard && ui.take_paste_request() {
            let text = self
                .clipboard
                .as_mut()
                .map(|c| c.read(&mut self.client))
                .unwrap_or_default();
            ui.paste(&text);
        }
        if let Some(cursor) = ui.take_cursor() {
            self.client.set_cursor(match cursor {
                crate::Cursor::Default => CursorShape::Default,
                crate::Cursor::Pointer => CursorShape::Pointer,
                crate::Cursor::Text => CursorShape::Text,
                crate::Cursor::ColResize => CursorShape::Other(30),
                crate::Cursor::RowResize => CursorShape::Other(31),
            });
        }
        ui.update();
        if ui.take_changed() {
            self.pending = true;
        }
        if self.pending && !self.awaiting {
            self.draw(ui)?;
        }
        Ok(rest)
    }

    /// Take the GPU made on its thread, once it is: frames go to it from
    /// the next one on, drawn whole; a device that could not be made is
    /// said, and the CPU goes on.
    #[cfg(feature = "gpu")]
    fn adopt_gpu(&mut self) {
        if !self
            .starting
            .as_mut()
            .is_some_and(|(pending, _)| pending.ready())
        {
            return;
        }
        let Some((pending, said)) = self.starting.take() else {
            return;
        };
        match pending.finish() {
            Ok(gpu) => {
                self.presenter = Some(crate::gpu::present::Presenter::new(
                    gpu,
                    &mut self.client,
                    &said,
                ));
                self.pending = true;
            }
            Err(why) => crate::gpu::present::warn(&format!("tole: drawing on the CPU: {why}")),
        }
    }

    /// The GPU's frame drawn while the program went on, committed once it
    /// is; its callback, when it came, letting the next frame be drawn.
    #[cfg(feature = "gpu")]
    fn settle_gpu(&mut self) {
        let Some(presenter) = self.presenter.as_mut() else {
            return;
        };
        if let Err(why) = presenter.settle(&mut self.client, self.surface) {
            crate::gpu::present::warn(&format!(
                "tole: the GPU failed ({why}); drawing on the CPU from now on"
            ));
            if let Some(presenter) = self.presenter.take() {
                presenter.close(&mut self.client);
            }
            self.frame = None;
            self.awaiting = false;
            self.pending = true;
            return;
        }
        if presenter.take_frame() {
            self.awaiting = false;
        }
    }

    /// A pointer event: this window's go to the `Ui`.
    fn pointer(&mut self, ui: &mut Ui, pointer: PointerEvent, rest: &mut Vec<Event>) {
        let ours = |surface: SurfaceId| surface == self.surface;
        match pointer {
            PointerEvent::Enter { surface, x, y } | PointerEvent::Motion { surface, x, y }
                if ours(surface) =>
            {
                if self.options.resize_edges {
                    let (edge, shape) = if self.client.is_maximized(self.surface) {
                        (0, CursorShape::Default)
                    } else {
                        edge_at(x as f32, y as f32, self.size.unwrap_or((1, 1)))
                    };
                    if edge != self.edge {
                        self.edge = edge;
                        self.client.set_cursor(shape);
                    }
                }
                ui.pointer_move(x as f32, y as f32);
            }
            PointerEvent::Button {
                surface,
                button,
                pressed: true,
                ..
            } if ours(surface) && button == BUTTON_LEFT && self.edge != 0 => {
                self.client.drag_window(self.surface, self.edge);
            }
            PointerEvent::Leave { surface } if ours(surface) => ui.pointer_leave(),
            PointerEvent::Button {
                surface,
                button,
                pressed,
                ..
            } if ours(surface) && button == BUTTON_LEFT => {
                if pressed {
                    ui.pointer_press();
                } else {
                    ui.pointer_release();
                }
            }
            PointerEvent::Axis {
                surface, vertical, ..
            } if ours(surface) => ui.pointer_wheel(vertical as f32),
            _ => rest.push(Event::Pointer(pointer)),
        }
    }

    /// A keyboard event: this window's keys go to the `Ui`, and those it
    /// does not take back to the program.
    fn keyboard(&mut self, ui: &mut Ui, keyboard: KeyboardEvent, rest: &mut Vec<Event>) {
        match &keyboard {
            KeyboardEvent::Enter(surface) if *surface == self.surface => {
                ui.set_window_active(true);
            }
            KeyboardEvent::Leave(surface) if *surface == self.surface => {
                ui.set_window_active(false);
            }
            KeyboardEvent::Modifiers(modifiers) => {
                ui.set_modifiers(modifiers.shift, modifiers.control);
            }
            KeyboardEvent::Key(key) if key.surface.is_none_or(|s| s == self.surface) => {
                if key.pressed {
                    self.key_serial = key.serial;
                }
                let event = crate::KeyEvent {
                    key: std::rc::Rc::from(key.keysym.unwrap_or("")),
                    text: std::rc::Rc::from(key.text.as_str()),
                    shift: key.modifiers.shift,
                    control: key.modifiers.control,
                    alt: key.modifiers.alt,
                    repeat: key.repeat,
                };
                let taken = if key.pressed {
                    ui.key_press(&event)
                } else {
                    ui.key_release(&event)
                };
                if taken {
                    return;
                }
            }
            _ => {}
        }
        rest.push(Event::Keyboard(keyboard));
    }

    /// Draw what changed and hand it over, counting the frames handed.
    fn draw(&mut self, ui: &mut Ui) -> Result<(), Error> {
        let started = std::time::Instant::now();
        let was = self.awaiting;
        self.awaiting = false;
        let drawn = self.draw_frame(ui);
        if self.awaiting {
            let took = started.elapsed();
            self.stats.frames += 1;
            self.stats.last = took;
            self.stats.total += took;
            self.stats.at = Some(std::time::Instant::now());
        } else {
            self.awaiting = was;
        }
        drawn
    }

    /// [`Window::draw`]'s work.
    fn draw_frame(&mut self, ui: &mut Ui) -> Result<(), Error> {
        let Some(size) = self.size else {
            return Ok(());
        };
        #[cfg(feature = "gpu")]
        if let Some(presenter) = self.presenter.as_mut() {
            let drawn = presenter.draw(
                ui,
                &mut self.client,
                self.surface,
                (size, self.scale),
                self.options.background,
            );
            match drawn {
                Ok(Some(drawn)) => {
                    let _ = ui.take_changed();
                    self.pending = false;
                    self.awaiting |= drawn;
                    return Ok(());
                }
                // Every buffer is the compositor's: drawn when one comes
                // back, which wakes the dispatch.
                Ok(None) => return Ok(()),
                Err(why) => {
                    crate::gpu::present::warn(&format!(
                        "tole: the GPU failed ({why}); drawing on the CPU from now on"
                    ));
                    if let Some(presenter) = self.presenter.take() {
                        presenter.close(&mut self.client);
                    }
                    self.frame = None;
                }
            }
        }
        let scale = self.scale as f32;
        let fits = self
            .frame
            .as_ref()
            .is_some_and(|frame| frame.pixel_size() == (size.0 * self.scale, size.1 * self.scale));
        if !fits {
            self.frame = Frame::new(size, scale, self.options.background);
        }
        let Some(frame) = self.frame.as_mut() else {
            return Ok(());
        };
        let damage = ui.render_into(frame);
        let _ = ui.take_changed();
        self.pending = false;
        if damage.is_empty() {
            return Ok(());
        }
        let pixels = frame.pixel_size();
        let drawn = self.client.draw_damaged(
            self.surface,
            frame.pixmap().data(),
            pixels,
            self.scale,
            &damage,
        )?;
        if drawn {
            self.client.request_frame(self.surface);
            self.awaiting = true;
        }
        Ok(())
    }
}

/// Start the GPU for a window when `choice` asks for it: the compositor
/// asked what dmabufs it takes and which device is its own, and that device
/// made on a thread, which wakes the window's loop when done. `None` for
/// the CPU.
#[cfg(feature = "gpu")]
fn start_gpu(
    choice: crate::gpu::Choice,
    client: &mut Client,
) -> Option<(
    crate::gpu::Pending,
    crate::gpu::present::Feedback,
    Vec<Event>,
)> {
    use crate::gpu::Choice;
    let hardware_only = match choice.resolve() {
        Choice::Gpu => false,
        Choice::Auto => true,
        Choice::Cpu | Choice::FromEnv => return None,
    };
    let (said, events) = crate::gpu::present::feedback(client);
    let waker = client.waker().ok();
    let pending = crate::gpu::Gpu::start(
        hardware_only,
        said.main,
        Box::new(move || {
            if let Some(waker) = waker {
                waker.wake();
            }
        }),
    );
    Some((pending, said, events))
}
