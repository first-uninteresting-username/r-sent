//! Wayland connection, globals, xdg toplevel, seat input, scaling, shm buffers, event loop (§6).

use std::num::NonZeroU32;

use smithay_client_toolkit::compositor::{
    CompositorHandler, CompositorState, FrameCallbackData, Region,
};
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::{GlobalList, registry_queue_init};
use smithay_client_toolkit::reexports::client::protocol::{
    wl_keyboard::WlKeyboard, wl_output::Transform, wl_output::WlOutput, wl_pointer::WlPointer,
    wl_seat::WlSeat, wl_shm, wl_surface::WlSurface,
};
use smithay_client_toolkit::reexports::client::{Connection, Proxy, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::{self, WpFractionalScaleManagerV1},
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::{
    wp_viewport::{self, WpViewport},
    wp_viewporter::{self, WpViewporter},
};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keymap, Keysym, Modifiers, RawModifiers,
};
use smithay_client_toolkit::seat::pointer::{
    AxisScroll, PointerEvent, PointerEventKind, PointerHandler,
};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::xdg::XdgShell;
use smithay_client_toolkit::shell::xdg::window::{
    Window, WindowConfigure, WindowDecorations, WindowHandler,
};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_dispatch2, delegate_registry, registry_handlers};
use xkbcommon::xkb;

use crate::app::{App, Effect, Source};
use crate::config::{Action, Button};
use crate::error::Error;
use crate::font::Fonts;
use crate::render::{self, Framebuffer};

/// Logical size used until the compositor suggests one.
pub const DEFAULT_SIZE: (u32, u32) = (1280, 720);

// ---------------------------------------------------------------------------------------------
// Pure helpers.

/// The surface scale factor: fractional (in 120ths) if known, else the integer buffer scale.
pub fn scale_factor(fractional_120: Option<u32>, integer: i32) -> f64 {
    match fractional_120 {
        Some(f) if f > 0 => f64::from(f) / 120.0,
        _ => f64::from(integer.max(1)),
    }
}

/// Physical size `round(w * s) × round(h * s)`, at least 1×1.
pub fn physical_size(w: u32, h: u32, s: f64) -> (u32, u32) {
    let r = |v: u32| ((f64::from(v) * s).round() as u32).max(1);
    (r(w), r(h))
}

/// Applies a configure's suggested size: `0` (unset) keeps the previous size.
pub fn configured_size(
    new: (Option<NonZeroU32>, Option<NonZeroU32>),
    last: (u32, u32),
) -> (u32, u32) {
    (
        new.0.map_or(last.0, NonZeroU32::get),
        new.1.map_or(last.1, NonZeroU32::get),
    )
}

pub const BTN_LEFT: u32 = 0x110;
pub const BTN_RIGHT: u32 = 0x111;
pub const BTN_MIDDLE: u32 = 0x112;

/// Maps an evdev button code to a configurable button.
pub fn map_button(code: u32) -> Option<Button> {
    match code {
        BTN_LEFT => Some(Button::Left),
        BTN_MIDDLE => Some(Button::Middle),
        BTN_RIGHT => Some(Button::Right),
        _ => None,
    }
}

/// Logical units of continuous scrolling per step.
pub const CONTINUOUS_STEP: f64 = 50.0;

/// Converts vertical axis events into scroll steps (positive = down).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ScrollAccumulator {
    value120: i32,
    continuous: f64,
}

impl ScrollAccumulator {
    /// Feeds one frame of vertical axis data; returns the number of whole steps.
    pub fn feed(&mut self, value120: i32, discrete: i32, absolute: f64, stop: bool) -> i32 {
        let steps = if value120 != 0 {
            self.value120 += value120;
            let steps = self.value120 / 120;
            self.value120 -= steps * 120;
            steps
        } else if discrete != 0 {
            discrete
        } else if absolute != 0.0 {
            self.continuous += absolute;
            let steps = (self.continuous / CONTINUOUS_STEP).trunc();
            self.continuous -= steps * CONTINUOUS_STEP;
            steps as i32
        } else {
            0
        };
        if stop {
            self.continuous = 0.0;
        }
        steps
    }

    fn feed_axis(&mut self, a: &AxisScroll) -> i32 {
        self.feed(a.value120, a.discrete, a.absolute, a.stop)
    }
}

// ---------------------------------------------------------------------------------------------
// User data for protocol objects SCTK does not wrap.

struct FractionalScaleData;
struct NoEvents;

impl Dispatch2<WpFractionalScaleV1, State> for FractionalScaleData {
    fn event(
        &self,
        state: &mut State,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &Connection,
        qh: &QueueHandle<State>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event
            && state.fractional_scale != Some(scale)
        {
            state.fractional_scale = Some(scale);
            state.scale_changed(qh);
        }
    }
}

impl Dispatch2<WpFractionalScaleManagerV1, State> for NoEvents {
    fn event(
        &self,
        _: &mut State,
        _: &WpFractionalScaleManagerV1,
        _: wp_fractional_scale_manager_v1::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch2<WpViewporter, State> for NoEvents {
    fn event(
        &self,
        _: &mut State,
        _: &WpViewporter,
        _: wp_viewporter::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch2<WpViewport, State> for NoEvents {
    fn event(
        &self,
        _: &mut State,
        _: &WpViewport,
        _: wp_viewport::Event,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}

// ---------------------------------------------------------------------------------------------
// State.

struct Fractional {
    scale: WpFractionalScaleV1,
    viewport: WpViewport,
}

struct State {
    registry: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    shm: Shm,
    _xdg: XdgShell,
    window: Window,
    fractional: Option<Fractional>,
    pool: SlotPool,
    /// Buffers with their physical size.
    buffers: Vec<(Buffer, (u32, u32))>,
    loop_handle: LoopHandle<'static, State>,
    qh: QueueHandle<State>,

    keyboard: Option<WlKeyboard>,
    pointer: Option<WlPointer>,
    xkb_context: xkb::Context,
    keymap: Option<xkb::Keymap>,
    layout: u32,
    scroll: ScrollAccumulator,

    /// Logical size.
    size: (u32, u32),
    integer_scale: i32,
    fractional_scale: Option<u32>,
    configured: bool,
    frame_pending: bool,
    dirty: bool,

    app: App,
    fonts: Fonts,
    fb: Framebuffer,
}

/// Connects to the compositor, shows the presentation and runs until quit.
pub fn run(mut app: App) -> Result<(), Error> {
    let conn = Connection::connect_to_env().map_err(|e| Error::Connect(e.to_string()))?;
    let (globals, event_queue) =
        registry_queue_init::<State>(&conn).map_err(|e| Error::Connect(e.to_string()))?;
    let qh = event_queue.handle();

    let compositor =
        CompositorState::bind(&globals, &qh).map_err(|_| Error::MissingGlobal("wl_compositor"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|_| Error::MissingGlobal("wl_shm"))?;
    let xdg = XdgShell::bind(&globals, &qh).map_err(|_| Error::MissingGlobal("xdg_wm_base"))?;
    let fractional_manager = bind_optional::<WpFractionalScaleManagerV1>(&globals, &qh);
    let viewporter = bind_optional::<WpViewporter>(&globals, &qh);

    // Fonts and images: errors are fatal.
    let fonts = Fonts::new(&app.config.fonts)?;
    crate::app::load_images(&mut app.slides, &app.config)?;

    let surface = compositor.create_surface(&qh);
    let fractional = match (&fractional_manager, &viewporter) {
        (Some(m), Some(v)) => Some(Fractional {
            scale: m.get_fractional_scale(&surface, &qh, FractionalScaleData),
            viewport: v.get_viewport(&surface, &qh, NoEvents),
        }),
        _ => None,
    };
    let window = xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_title(match &app.source {
        Source::File(p) => format!(
            "r-sent: {}",
            p.file_name().unwrap_or(p.as_os_str()).to_string_lossy()
        ),
        Source::Stdin => "r-sent".to_owned(),
    });
    window.set_app_id("r-sent");
    if app.config.fullscreen {
        window.set_fullscreen(None);
    }
    window.commit();

    let pool = SlotPool::new(DEFAULT_SIZE.0 as usize * DEFAULT_SIZE.1 as usize * 4, &shm)
        .map_err(|e| Error::Wayland(format!("cannot create shm pool: {e}")))?;

    let mut event_loop: EventLoop<State> = EventLoop::try_new()
        .map_err(|e| Error::Wayland(format!("cannot create event loop: {e}")))?;
    WaylandSource::new(conn.clone(), event_queue)
        .insert(event_loop.handle())
        .map_err(|e| Error::Wayland(format!("cannot register connection: {e}")))?;

    let mut state = State {
        registry: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        compositor,
        shm,
        _xdg: xdg,
        window,
        fractional,
        pool,
        buffers: Vec::new(),
        loop_handle: event_loop.handle(),
        qh: qh.clone(),
        keyboard: None,
        pointer: None,
        xkb_context: xkb::Context::new(xkb::CONTEXT_NO_FLAGS),
        keymap: None,
        layout: 0,
        scroll: ScrollAccumulator::default(),
        size: DEFAULT_SIZE,
        integer_scale: 1,
        fractional_scale: None,
        configured: false,
        frame_pending: false,
        dirty: true,
        app,
        fonts,
        fb: Framebuffer::new(0, 0),
    };

    while state.app.running {
        event_loop
            .dispatch(None, &mut state)
            .map_err(|e| Error::Wayland(e.to_string()))?;
        if let Some(e) = conn.protocol_error() {
            return Err(Error::Wayland(format!(
                "protocol error {} on {}@{}: {}",
                e.code, e.object_interface, e.object_id, e.message
            )));
        }
    }

    state.shutdown();
    drop(state);
    if let Some(m) = fractional_manager {
        m.destroy();
    }
    if let Some(v) = viewporter {
        v.destroy();
    }
    conn.flush().map_err(|e| Error::Wayland(e.to_string()))?;
    Ok(())
}

fn bind_optional<I>(globals: &GlobalList, qh: &QueueHandle<State>) -> Option<I>
where
    I: Proxy + 'static,
    State: smithay_client_toolkit::reexports::client::Dispatch<I, NoEvents>,
{
    globals.bind::<I, State, NoEvents>(qh, 1..=1, NoEvents).ok()
}

impl State {
    fn surface(&self) -> &WlSurface {
        self.window.wl_surface()
    }

    fn scale(&self) -> f64 {
        let frac = if self.fractional.is_some() {
            self.fractional_scale
        } else {
            None
        };
        scale_factor(frac, self.integer_scale)
    }

    fn physical(&self) -> (u32, u32) {
        let s = self.scale();
        if self.fractional.is_some() {
            physical_size(self.size.0, self.size.1, s)
        } else {
            // Integer scale: the buffer must be an exact multiple of the logical size.
            let s = self.integer_scale.max(1) as u32;
            (self.size.0.max(1) * s, self.size.1.max(1) * s)
        }
    }

    fn scale_changed(&mut self, qh: &QueueHandle<State>) {
        self.app.current().drop_scaled();
        self.request_redraw(qh);
    }

    /// Schedules a redraw on the next frame callback.
    fn request_redraw(&mut self, qh: &QueueHandle<State>) {
        self.dirty = true;
        if !self.configured || self.frame_pending {
            return;
        }
        let surface = self.surface().clone();
        surface.frame(qh, FrameCallbackData(surface.clone()));
        surface.commit();
        self.frame_pending = true;
    }

    fn draw(&mut self) {
        self.dirty = false;
        let (pw, ph) = self.physical();
        self.fonts.set_surface_scale(self.scale());
        self.fb.resize(pw, ph);
        let idx = self.app.idx;
        render::draw_slide(
            &mut self.fb,
            &mut self.app.slides[idx],
            &mut self.fonts,
            &self.app.config,
        );

        let format = if self.shm.formats().contains(&wl_shm::Format::Xrgb8888) {
            wl_shm::Format::Xrgb8888
        } else {
            wl_shm::Format::Argb8888
        };
        let stride = pw as i32 * 4;

        // Double buffering: reuse a released buffer of the right size, never one the
        // compositor still holds.
        self.buffers.retain(|(_, size)| *size == (pw, ph));
        let free = self
            .buffers
            .iter()
            .position(|(b, _)| b.canvas(&mut self.pool).is_some());
        let index = match free {
            Some(i) => i,
            None => {
                let buffer = match self
                    .pool
                    .create_buffer(pw as i32, ph as i32, stride, format)
                {
                    Ok((b, _)) => b,
                    Err(e) => {
                        crate::error::report(&Error::Wayland(format!(
                            "cannot allocate buffer: {e}"
                        )));
                        return;
                    }
                };
                if self.buffers.len() >= 2 {
                    self.buffers.remove(0);
                }
                self.buffers.push((buffer, (pw, ph)));
                self.buffers.len() - 1
            }
        };
        let buffer = &self.buffers[index].0;
        if let Some(canvas) = buffer.canvas(&mut self.pool) {
            self.fb.write_to(canvas);
        }

        let surface = self.window.wl_surface().clone();
        if let Err(e) = buffer.attach_to(&surface) {
            crate::error::report(&Error::Wayland(format!("cannot attach buffer: {e}")));
            return;
        }
        surface.damage_buffer(0, 0, pw as i32, ph as i32);
        match &self.fractional {
            Some(f) => {
                surface.set_buffer_scale(1);
                f.viewport
                    .set_destination(self.size.0 as i32, self.size.1 as i32);
            }
            None => surface.set_buffer_scale(self.integer_scale.max(1)),
        }
        if let Ok(region) = Region::new(&self.compositor) {
            region.add(0, 0, self.size.0 as i32, self.size.1 as i32);
            surface.set_opaque_region(Some(region.wl_region()));
        }
        surface.commit();
    }

    fn run_actions(&mut self, actions: Vec<Action>) {
        let qh = self.qh.clone();
        for action in actions {
            match self.app.perform(action) {
                Effect::None => {}
                Effect::Redraw => self.request_redraw(&qh),
                Effect::ToggleFullscreen => {
                    if self.app.fullscreen {
                        self.window.unset_fullscreen();
                    } else {
                        self.window.set_fullscreen(None);
                    }
                }
            }
        }
    }

    /// Keysym at shift level 0 of the active layout for an evdev key code.
    fn level0_keysym(&self, raw_code: u32, fallback: Keysym) -> Keysym {
        let Some(keymap) = &self.keymap else {
            return fallback;
        };
        let code = xkb::Keycode::new(raw_code + 8);
        let layouts = keymap.num_layouts_for_key(code);
        if layouts == 0 {
            return fallback;
        }
        keymap
            .key_get_syms_by_level(code, self.layout % layouts, 0)
            .first()
            .copied()
            .unwrap_or(fallback)
    }

    fn key(&mut self, event: &KeyEvent) {
        let sym = self.level0_keysym(event.raw_code, event.keysym);
        let actions = self
            .app
            .config
            .keys
            .iter()
            .filter(|b| b.keysym == sym)
            .map(|b| b.action)
            .collect();
        self.run_actions(actions);
    }

    fn button(&mut self, button: Button, times: u32) {
        let actions: Vec<Action> = self
            .app
            .config
            .mouse
            .iter()
            .filter(|b| b.button == button)
            .map(|b| b.action)
            .collect();
        for _ in 0..times {
            self.run_actions(actions.clone());
            if !self.app.running {
                break;
            }
        }
    }

    fn shutdown(&mut self) {
        if let Some(k) = self.keyboard.take()
            && k.version() >= 3
        {
            k.release();
        }
        if let Some(p) = self.pointer.take()
            && p.version() >= 3
        {
            p.release();
        }
        if let Some(f) = self.fractional.take() {
            f.viewport.destroy();
            f.scale.destroy();
        }
        self.buffers.clear();
    }
}

// ---------------------------------------------------------------------------------------------
// Handlers.

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &WlSurface,
        new_factor: i32,
    ) {
        if new_factor != self.integer_scale {
            self.integer_scale = new_factor.max(1);
            if self.fractional.is_none() || self.fractional_scale.is_none() {
                self.scale_changed(qh);
            }
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlSurface, _: u32) {
        self.frame_pending = false;
        if self.dirty {
            self.draw();
        }
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: &WlOutput,
    ) {
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlOutput) {}
}

impl WindowHandler for State {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.app.running = false;
    }

    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        self.app.fullscreen = configure.is_fullscreen();
        let size = configured_size(configure.new_size, self.size);
        let resized = size != self.size;
        self.size = size;
        if resized {
            self.app.current().drop_scaled();
        }
        if !self.configured || resized {
            // The acked size must be matched by the next commit, so draw right away.
            self.configured = true;
            self.draw();
        } else if self.dirty {
            self.request_redraw(qh);
        }
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let repeat =
                Box::new(|state: &mut State, _: &WlKeyboard, event: KeyEvent| state.key(&event));
            match self.seat_state.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                self.loop_handle.clone(),
                repeat,
            ) {
                Ok(k) => self.keyboard = Some(k),
                Err(e) => eprintln!("r-sent: cannot use keyboard: {e}"),
            }
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(p) => self.pointer = Some(p),
                Err(e) => eprintln!("r-sent: cannot use pointer: {e}"),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Keyboard
            && let Some(k) = self.keyboard.take()
            && k.version() >= 3
        {
            k.release();
        }
        if capability == Capability::Pointer
            && let Some(p) = self.pointer.take()
            && p.version() >= 3
        {
            p.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat) {}
}

impl KeyboardHandler for State {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: &WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: &WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(&event);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(&event);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: u32,
        _: Modifiers,
        _: RawModifiers,
        layout: u32,
    ) {
        self.layout = layout;
    }

    fn update_keymap(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        keymap: Keymap<'_>,
    ) {
        self.keymap = xkb::Keymap::new_from_string(
            &self.xkb_context,
            keymap.as_string(),
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        );
    }
}

impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match &event.kind {
                PointerEventKind::Press { button, .. } => {
                    if let Some(b) = map_button(*button) {
                        self.button(b, 1);
                    }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let steps = self.scroll.feed_axis(vertical);
                    if steps > 0 {
                        self.button(Button::ScrollDown, steps.unsigned_abs());
                    } else if steps < 0 {
                        self.button(Button::ScrollUp, steps.unsigned_abs());
                    }
                }
                _ => {}
            }
        }
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(State);
delegate_dispatch2!(State);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_and_physical_size() {
        assert_eq!(scale_factor(None, 1), 1.0);
        assert_eq!(scale_factor(None, 2), 2.0);
        assert_eq!(scale_factor(None, 0), 1.0);
        assert_eq!(scale_factor(Some(180), 1), 1.5);
        assert_eq!(scale_factor(Some(150), 2), 1.25);
        assert_eq!(scale_factor(Some(0), 2), 2.0);

        assert_eq!(physical_size(1280, 720, 1.0), (1280, 720));
        assert_eq!(physical_size(1280, 720, 1.5), (1920, 1080));
        // round(), not floor(): 1001 * 1.25 = 1251.25, 333 * 1.5 = 499.5 → 500.
        assert_eq!(physical_size(1001, 333, 1.25), (1251, 416));
        assert_eq!(physical_size(333, 1, 1.5), (500, 2));
        assert_eq!(physical_size(0, 0, 2.0), (1, 1));
    }

    #[test]
    fn configure_size() {
        let nz = NonZeroU32::new;
        assert_eq!(configured_size((None, None), DEFAULT_SIZE), (1280, 720));
        assert_eq!(
            configured_size((nz(800), nz(600)), DEFAULT_SIZE),
            (800, 600)
        );
        assert_eq!(configured_size((None, None), (800, 600)), (800, 600));
    }

    #[test]
    fn evdev_buttons() {
        assert_eq!(map_button(0x110), Some(Button::Left));
        assert_eq!(map_button(0x111), Some(Button::Right));
        assert_eq!(map_button(0x112), Some(Button::Middle));
        assert_eq!(map_button(0x113), None);
    }

    #[test]
    fn scroll_value120() {
        let mut s = ScrollAccumulator::default();
        assert_eq!(s.feed(120, 1, 15.0, false), 1);
        assert_eq!(s.feed(-240, -2, -30.0, false), -2);
        // High-resolution wheels: 4 × 30 = one notch.
        assert_eq!(s.feed(30, 0, 3.75, false), 0);
        assert_eq!(s.feed(30, 0, 3.75, false), 0);
        assert_eq!(s.feed(30, 0, 3.75, false), 0);
        assert_eq!(s.feed(30, 0, 3.75, false), 1);
        assert_eq!(s.feed(-60, 0, -7.5, false), 0);
        assert_eq!(s.feed(-60, 0, -7.5, false), -1);
    }

    #[test]
    fn scroll_discrete() {
        let mut s = ScrollAccumulator::default();
        assert_eq!(s.feed(0, 1, 10.0, false), 1);
        assert_eq!(s.feed(0, -3, -30.0, false), -3);
    }

    #[test]
    fn scroll_continuous() {
        let mut s = ScrollAccumulator::default();
        assert_eq!(s.feed(0, 0, 20.0, false), 0);
        assert_eq!(s.feed(0, 0, 20.0, false), 0);
        assert_eq!(s.feed(0, 0, 20.0, false), 1);
        assert_eq!(s.feed(0, 0, 110.0, false), 2);
        // 10 + 110 - 100 = 20 left; stop resets.
        assert_eq!(s.feed(0, 0, 0.0, true), 0);
        assert_eq!(s.feed(0, 0, 40.0, false), 0);
        assert_eq!(s.feed(0, 0, -45.0, false), 0);
        assert_eq!(s.feed(0, 0, -50.0, false), -1);
    }
}
