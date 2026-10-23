//! A window: a tree shown in a Wayland toplevel through
//! `compositor-toolkit` (`docs/UI-FRAMEWORK.md` §4.10), on Ferrix under
//! hyprix and on any Wayland desktop -- the toolkit is pure Rust over the
//! socket.
//!
//! [`Window::dispatch`] waits for what happens, feeds the pointer to the
//! `Ui`, sizes the root to the window, and draws what changed: only the
//! damaged parts are drawn again and handed over, paced by the
//! compositor's frame callbacks. What tole does not take -- keys, timers,
//! a close request -- comes back as the toolkit's events.

use std::time::Duration;

use compositor_toolkit::{
    Client, Error, Event, KeyboardEvent, PointerEvent, SurfaceId, ToplevelOptions,
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
}

impl Window {
    /// Connect to `$WAYLAND_DISPLAY` and open a toplevel showing the tree
    /// whose root is `root`.
    ///
    /// # Errors
    ///
    /// No compositor, or one without `xdg_wm_base`.
    pub fn open(root: Common, options: WindowOptions) -> Result<Self, Error> {
        let mut client = Client::connect()?;
        let surface = client.toplevel(&ToplevelOptions {
            title: options.title.clone(),
            app_id: options.app_id.clone(),
            size: options.size,
            ..ToplevelOptions::default()
        })?;
        Ok(Window {
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
        })
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
        let events = self.client.dispatch(timeout)?;
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
                other => rest.push(other),
            }
        }
        if ui.take_changed() {
            self.pending = true;
        }
        if self.pending && !self.awaiting {
            self.draw(ui)?;
        }
        Ok(rest)
    }

    /// A pointer event: this window's go to the `Ui`.
    fn pointer(&mut self, ui: &mut Ui, pointer: PointerEvent, rest: &mut Vec<Event>) {
        let ours = |surface: SurfaceId| surface == self.surface;
        match pointer {
            PointerEvent::Enter { surface, x, y } | PointerEvent::Motion { surface, x, y }
                if ours(surface) =>
            {
                ui.pointer_move(x as f32, y as f32);
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
            KeyboardEvent::Key(key) if key.surface.is_none_or(|s| s == self.surface) => {
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

    /// Draw what changed and hand it over.
    fn draw(&mut self, ui: &mut Ui) -> Result<(), Error> {
        let Some(size) = self.size else {
            return Ok(());
        };
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
