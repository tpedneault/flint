//! The window and its event loop.
//!
//! The daemon's logic lives outside this crate and drives the window
//! through two small surfaces: it implements [`Controller`] to hear about
//! the window being ready, its own events arriving (hotkey presses, client
//! requests), and frames being shown; and it acts through [`Ui`], which
//! can show, hide, set the rows and the footer, and apply settings.
//!
//! How the window is shown depends on the platform, because Wayland
//! cannot hide a window:
//!
//! - **Windows and X11**: the window and its surface are created once,
//!   hidden, at startup. Showing is `set_visible(true)`, a focus request
//!   and a frame drawn at once, which is how the 50 ms budget is met.
//!   Hiding draws the cleared state into the hidden window, so the next
//!   show never flashes the previous query.
//! - **Wayland**: `set_visible` and `focus_window` do nothing there. The
//!   window is created on each show, with the client's activation token
//!   so KWin lets it take focus, and destroyed on hide. The GPU device,
//!   fonts and glyph atlas stay, so only the surface is made again.

use std::sync::Arc;
use std::time::{Duration, Instant};

use flint_config::Settings;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

use crate::gpu::{AdapterSummary, Gpu, Surface};
use crate::keys;
use crate::launcher::{Chip, FooterLine, Launcher, Outcome, Row};
use crate::layout::{self, Layout};
use crate::render::Renderer;
use crate::theme::{Palette, ORBIT_DARK};

/// A request to show the window.
#[derive(Debug, Clone)]
pub struct ShowRequest {
    /// Replaces the query line.
    pub query: Option<String>,
    /// Wayland's activation token from the client, if any.
    pub activation_token: Option<String>,
    /// What asked, for the timing log: `hotkey`, `toggle`, `show`.
    pub source: &'static str,
    /// When the request reached the daemon; the timing runs from here.
    pub requested_at: Instant,
}

/// A show that reached the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    /// What asked.
    pub source: &'static str,
    /// From the request to `present` returning.
    pub elapsed: Duration,
}

/// The daemon's side of the window.
pub trait Controller: 'static {
    /// The daemon's own events, sent through a [`Proxy`].
    type Event: Send + 'static;

    /// The window and GPU are ready, or couldn't be made (`Err` says why,
    /// in one line). Called once, on the event-loop thread, which is where
    /// a Windows hotkey must be registered.
    fn ready(&mut self, ui: &mut Ui<'_>, result: Result<(), String>);

    /// One of the daemon's events arrived.
    fn event(&mut self, event: Self::Event, ui: &mut Ui<'_>);

    /// A show reached the screen.
    fn shown(&mut self, shown: Shown, ui: &mut Ui<'_>);

    /// The query or its scope changed ([`Ui::query`], [`Ui::scope`]):
    /// search again and [`Ui::set_rows`]. Called on the event-loop thread
    /// within the keystroke, so it must answer within the frame budget.
    fn query_changed(&mut self, ui: &mut Ui<'_>);

    /// `Enter` on row `row`: run its first action.
    fn activate(&mut self, row: usize, ui: &mut Ui<'_>);

    /// `y` on row `row`: copy its primary text.
    fn copy(&mut self, row: usize, ui: &mut Ui<'_>);
}

/// Sends the daemon's events into the event loop from any thread.
pub struct Proxy<E: 'static>(EventLoopProxy<E>);

impl<E: 'static> Clone for Proxy<E> {
    fn clone(&self) -> Self {
        Proxy(self.0.clone())
    }
}

impl<E: 'static> Proxy<E> {
    /// Delivers `event`; `false` once the event loop has ended.
    pub fn send(&self, event: E) -> bool {
        self.0.send_event(event).is_ok()
    }
}

/// The event loop, before it runs.
pub struct Gui<E: 'static> {
    event_loop: EventLoop<E>,
}

impl<E: Send + 'static> Gui<E> {
    /// Connects to the display. Fails without one (no desktop session).
    pub fn new() -> Result<Self, String> {
        let event_loop = EventLoop::<E>::with_user_event()
            .build()
            .map_err(|e| format!("couldn't connect to the display: {e}"))?;
        // No timers, no polling: hidden, the daemon sleeps until an event.
        event_loop.set_control_flow(ControlFlow::Wait);
        Ok(Gui { event_loop })
    }

    /// A handle for sending events from other threads.
    pub fn proxy(&self) -> Proxy<E> {
        Proxy(self.event_loop.create_proxy())
    }

    /// Runs until [`Ui::exit`].
    pub fn run<C: Controller<Event = E>>(
        self,
        controller: C,
        settings: Settings,
    ) -> Result<(), String> {
        let mut app = App {
            controller,
            state: State::new(settings),
        };
        self.event_loop
            .run_app(&mut app)
            .map_err(|e| format!("the event loop failed: {e}"))
    }
}

/// What the controller may do to the window.
pub struct Ui<'a> {
    state: &'a mut State,
    event_loop: &'a ActiveEventLoop,
}

impl Ui<'_> {
    /// Shows the window (or, already shown, brings it forward with the
    /// new query).
    pub fn show(&mut self, request: ShowRequest) {
        self.state.show(self.event_loop, request);
    }

    /// Hides the window.
    pub fn hide(&mut self) {
        self.state.hide();
    }

    /// Hides if shown, shows if hidden; returns whether it is now shown.
    pub fn toggle(&mut self, request: ShowRequest) -> bool {
        if self.state.visible {
            self.state.hide();
        } else {
            self.state.show(self.event_loop, request);
        }
        self.state.visible
    }

    /// Whether the window is showing.
    pub fn visible(&self) -> bool {
        self.state.visible
    }

    /// New results for a new query: the first row selected.
    pub fn set_rows(&mut self, rows: Vec<Row>) {
        self.state.launcher.set_rows(rows);
        self.state.rows_presented = false;
        self.state.redraw();
    }

    /// Results changed without a new query (the daemon has kept the rows
    /// down to the selection in place): the selection stays.
    pub fn merge_rows(&mut self, rows: Vec<Row>) {
        self.state.launcher.merge_rows(rows);
        self.state.redraw();
    }

    /// Draws the current state into the hidden window, so the next show
    /// finds the rows' glyphs rasterised and its frame quick. Does nothing
    /// while shown, or on Wayland, where a hidden window doesn't exist.
    pub fn prepare(&mut self) {
        if !self.state.visible {
            self.state.draw();
        }
    }

    /// Whether the current rows have been on screen, so new results must
    /// not move them (the stable merge).
    pub fn rows_presented(&self) -> bool {
        self.state.rows_presented
    }

    /// The selected row.
    pub fn selected(&self) -> usize {
        self.state.launcher.selected()
    }

    /// The query text, without the scope.
    pub fn query(&self) -> &str {
        self.state.launcher.query()
    }

    /// The scope chip, if any.
    pub fn scope(&self) -> Option<&Chip> {
        self.state.launcher.scope()
    }

    /// Sets or clears the scope chip.
    pub fn set_scope(&mut self, scope: Option<Chip>) {
        self.state.launcher.set_scope(scope);
        self.state.redraw();
    }

    /// Replaces the query text.
    pub fn set_query(&mut self, query: String) {
        self.state.launcher.set_query(query);
        self.state.redraw();
    }

    /// Shows a message in the footer until the next edit or show.
    pub fn set_message(&mut self, message: FooterLine) {
        self.state.launcher.set_message(message);
        self.state.redraw();
    }

    /// Sets or clears the footer's standing notice.
    pub fn set_notice(&mut self, notice: Option<FooterLine>) {
        self.state.launcher.set_notice(notice);
        self.state.redraw();
    }

    /// Uses new settings (size, rows, hide on blur).
    pub fn apply_settings(&mut self, settings: Settings) {
        self.state.launcher.set_visible_rows(settings.rows as usize);
        self.state.launcher.set_escape_hides(settings.escape_hides);
        self.state.settings = settings;
        if self.state.visible {
            self.state.fit_window();
            self.state.redraw();
        }
    }

    /// Draws with `palette` and, when installed, `font` (else the
    /// monospace family Fenix would pick). Returns the family in use.
    pub fn set_appearance(&mut self, palette: Palette, font: Option<String>) -> String {
        self.state.palette = palette;
        self.state.font = font;
        if let Some(renderer) = &mut self.state.renderer {
            renderer.set_font(self.state.font.as_deref());
        }
        self.state.redraw();
        self.state
            .renderer
            .as_ref()
            .map(|r| r.font().to_owned())
            .unwrap_or_default()
    }

    /// The GPU adapter in use, once ready.
    pub fn adapter(&self) -> Option<AdapterSummary> {
        self.state.gpu.as_ref().map(|g| g.summary.clone())
    }

    /// Ends the event loop; [`Gui::run`] returns.
    pub fn exit(&mut self) {
        self.event_loop.exit();
    }
}

/// Whether the window lives for the daemon's life or only while shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Persistent,
    PerShow,
}

struct State {
    settings: Settings,
    palette: Palette,
    /// The font asked for; the renderer falls back when it isn't installed.
    font: Option<String>,
    mode: Mode,
    launcher: Launcher,
    gpu: Option<Gpu>,
    renderer: Option<Renderer>,
    window: Option<Arc<Window>>,
    surface: Option<Surface>,
    visible: bool,
    /// Focus arrived since the last show; only then does losing it hide.
    focused: bool,
    modifiers: ModifiersState,
    /// A show waiting for its first frame.
    pending: Option<(&'static str, Instant)>,
    /// Shows that reached the screen, for the controller.
    shown: Vec<Shown>,
    /// Key presses the controller must hear about.
    outcomes: Vec<Outcome>,
    /// The rows have been on screen since they were set.
    rows_presented: bool,
    started: bool,
}

impl State {
    fn new(settings: Settings) -> State {
        let mut launcher = Launcher::default();
        launcher.set_visible_rows(settings.rows as usize);
        launcher.set_escape_hides(settings.escape_hides);
        State {
            settings,
            palette: ORBIT_DARK,
            font: None,
            mode: if flint_wayland() {
                Mode::PerShow
            } else {
                Mode::Persistent
            },
            launcher,
            gpu: None,
            renderer: None,
            window: None,
            surface: None,
            visible: false,
            focused: false,
            modifiers: ModifiersState::empty(),
            pending: None,
            shown: Vec::new(),
            outcomes: Vec::new(),
            rows_presented: false,
            started: false,
        }
    }

    fn layout(&self) -> Layout {
        Layout::new(self.settings.width, self.settings.rows)
    }

    fn attributes(&self, visible: bool, token: Option<String>) -> WindowAttributes {
        let l = self.layout();
        let attributes = Window::default_attributes()
            .with_title("Flint")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(visible)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_inner_size(LogicalSize::new(l.window.w, l.window.h));
        platform_attributes(attributes, token)
    }

    /// Creates the GPU, the renderer and (on Windows and X11) the hidden
    /// window.
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let mut descriptor = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(
            event_loop.owned_display_handle(),
        ));
        // DX12 on Windows: measured against Vulkan on the same machine it
        // starts faster and uses about 20 MB less, which is the difference
        // between meeting the 120 MB budget and not; and its fallback
        // adapter is WARP, which every Windows has. `WGPU_BACKEND` still
        // overrides it, for diagnosing a driver.
        if cfg!(windows) {
            descriptor.backends = wgpu::Backends::DX12;
        }
        let instance = wgpu::Instance::new(descriptor.with_env());
        match self.mode {
            Mode::Persistent => {
                let window = Arc::new(
                    event_loop
                        .create_window(self.attributes(false, None))
                        .map_err(|e| format!("couldn't create the window: {e}"))?,
                );
                let raw = instance
                    .create_surface(Arc::clone(&window))
                    .map_err(|e| format!("couldn't create a surface: {e}"))?;
                let gpu = Gpu::new(instance, Some(&raw))?;
                let surface = Surface::new(&gpu, Arc::clone(&window), Some(raw), None)?;
                self.renderer = Some(Renderer::new(&gpu, surface.config.format));
                self.gpu = Some(gpu);
                self.window = Some(window);
                self.surface = Some(surface);
                // Draw once while hidden so the first show has a frame.
                self.draw();
            }
            Mode::PerShow => {
                let gpu = Gpu::new(instance, None)?;
                // The format the compositor will offer is unknown until a
                // surface exists; Bgra8Unorm is what Wayland compositors
                // offer in practice, and a mismatch rebuilds the pipelines.
                self.renderer = Some(Renderer::new(&gpu, wgpu::TextureFormat::Bgra8Unorm));
                self.gpu = Some(gpu);
            }
        }
        Ok(())
    }

    fn show(&mut self, event_loop: &ActiveEventLoop, request: ShowRequest) {
        self.launcher.reset(request.query);
        self.pending = Some((request.source, request.requested_at));
        match self.mode {
            Mode::Persistent => {
                let Some(window) = self.window.clone() else {
                    return;
                };
                self.fit_window();
                window.set_visible(true);
                window.focus_window();
                if !self.visible {
                    self.focused = false;
                }
                self.visible = true;
                self.draw();
            }
            Mode::PerShow => {
                if self.window.is_some() {
                    // Already shown: a new query, and focus if the
                    // compositor allows it.
                    self.redraw();
                    return;
                }
                let attributes = self.attributes(true, request.activation_token);
                let window = match event_loop.create_window(attributes) {
                    Ok(window) => Arc::new(window),
                    Err(e) => {
                        self.launcher.set_notice(Some(FooterLine {
                            text: format!("couldn't create the window: {e}"),
                            error: true,
                        }));
                        return;
                    }
                };
                let (Some(gpu), Some(renderer)) = (&self.gpu, &mut self.renderer) else {
                    return;
                };
                match Surface::new(gpu, Arc::clone(&window), None, Some(renderer.format())) {
                    Ok(surface) => {
                        if surface.config.format != renderer.format() {
                            *renderer = Renderer::new(gpu, surface.config.format);
                            renderer.set_font(self.font.as_deref());
                        }
                        self.surface = Some(surface);
                    }
                    Err(_) => return,
                }
                self.focused = false;
                self.visible = true;
                window.request_redraw();
                self.window = Some(window);
            }
        }
    }

    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.pending = None;
        self.launcher.reset(None);
        match self.mode {
            Mode::Persistent => {
                if let Some(window) = &self.window {
                    window.set_visible(false);
                }
                self.draw();
            }
            Mode::PerShow => {
                self.surface = None;
                self.window = None;
            }
        }
    }

    /// Sizes the window from the settings and the display's scale, and
    /// (where the platform allows) centres it on its monitor.
    fn fit_window(&mut self) {
        let Some(window) = &self.window else { return };
        let scale = window.scale_factor();
        let l = self.layout();
        let size: PhysicalSize<u32> = LogicalSize::new(l.window.w, l.window.h).to_physical(scale);
        if window.inner_size() != size {
            let _ = window.request_inner_size(size);
            if let Some(surface) = &mut self.surface {
                surface.resize(size.width, size.height);
            }
        }
        if self.mode == Mode::Persistent {
            let monitor = window
                .current_monitor()
                .or_else(|| window.primary_monitor());
            if let Some(monitor) = monitor {
                let (p, s) = (monitor.position(), monitor.size());
                let outer = window.outer_size();
                let (x, y) = layout::place(
                    (p.x, p.y, s.width, s.height),
                    (outer.width.max(size.width), outer.height.max(size.height)),
                );
                window.set_outer_position(PhysicalPosition::new(x, y));
            }
        }
    }

    fn redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn draw(&mut self) {
        let (Some(gpu), Some(renderer), Some(surface), Some(window)) = (
            &self.gpu,
            &mut self.renderer,
            &mut self.surface,
            &self.window,
        ) else {
            return;
        };
        let scale = window.scale_factor() as f32;
        let layout = Layout::new(self.settings.width, self.settings.rows);
        let presented = renderer.draw(gpu, surface, &self.launcher, &layout, scale, &self.palette);
        if presented && self.visible {
            self.rows_presented = true;
            if let Some((source, at)) = self.pending.take() {
                self.shown.push(Shown {
                    source,
                    elapsed: at.elapsed(),
                });
            }
        }
    }

    fn window_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::RedrawRequested => self.draw(),
            WindowEvent::Resized(size) => {
                if let Some(surface) = &mut self.surface {
                    surface.resize(size.width, size.height);
                }
                self.redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                self.fit_window();
                self.redraw();
            }
            WindowEvent::Focused(true) => self.focused = true,
            WindowEvent::Focused(false) => {
                if self.visible && self.focused && self.settings.hide_on_blur {
                    self.hide();
                }
            }
            // Alt+F4 and the like hide; only `flint quit` ends the daemon.
            WindowEvent::CloseRequested => self.hide(),
            WindowEvent::ModifiersChanged(mods) => self.modifiers = mods.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                for press in keys::presses(&event.logical_key, self.modifiers) {
                    match self.launcher.press(press) {
                        Outcome::Nothing => {}
                        Outcome::Redraw => self.redraw(),
                        Outcome::Hide => {
                            self.hide();
                            break;
                        }
                        outcome => {
                            self.redraw();
                            // Several characters committed at once are one
                            // search, not several.
                            if self.outcomes.last() != Some(&outcome) {
                                self.outcomes.push(outcome);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

struct App<C: Controller> {
    controller: C,
    state: State,
}

impl<C: Controller> App<C> {
    /// Tells the controller what happened that it must act on: key
    /// presses that change the query or ask for an action, and shows that
    /// reached the screen.
    fn report(&mut self, event_loop: &ActiveEventLoop) {
        for outcome in std::mem::take(&mut self.state.outcomes) {
            let mut ui = Ui {
                state: &mut self.state,
                event_loop,
            };
            match outcome {
                Outcome::QueryChanged => self.controller.query_changed(&mut ui),
                Outcome::Activate(row) => self.controller.activate(row, &mut ui),
                Outcome::Copy(row) => self.controller.copy(row, &mut ui),
                Outcome::Nothing | Outcome::Redraw | Outcome::Hide => {}
            }
        }
        for shown in std::mem::take(&mut self.state.shown) {
            let mut ui = Ui {
                state: &mut self.state,
                event_loop,
            };
            self.controller.shown(shown, &mut ui);
        }
    }
}

impl<C: Controller> ApplicationHandler<C::Event> for App<C> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.started {
            return;
        }
        self.state.started = true;
        let result = self.state.start(event_loop);
        let mut ui = Ui {
            state: &mut self.state,
            event_loop,
        };
        self.controller.ready(&mut ui, result);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: C::Event) {
        let mut ui = Ui {
            state: &mut self.state,
            event_loop,
        };
        self.controller.event(event, &mut ui);
        self.report(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.state.window.as_ref().map(|w| w.id()) != Some(id) {
            return;
        }
        self.state.window_event(event);
        self.report(event_loop);
    }
}

/// The same rule as `flint_platform::display::is_wayland`, which winit
/// also follows to pick its backend.
fn flint_wayland() -> bool {
    !cfg!(windows) && std::env::var_os("WAYLAND_DISPLAY").is_some_and(|d| !d.is_empty())
}

#[cfg(windows)]
fn platform_attributes(attributes: WindowAttributes, _token: Option<String>) -> WindowAttributes {
    use winit::platform::windows::WindowAttributesExtWindows;
    // No taskbar button: Flint is summoned, not switched to.
    attributes.with_skip_taskbar(true)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_attributes(attributes: WindowAttributes, token: Option<String>) -> WindowAttributes {
    use winit::platform::startup_notify::WindowAttributesExtStartupNotify;
    use winit::window::ActivationToken;
    // The app id / WM_CLASS the KWin rule in contrib/kde/ matches on.
    let attributes = if flint_wayland() {
        winit::platform::wayland::WindowAttributesExtWayland::with_name(
            attributes, "flint", "flint",
        )
    } else {
        winit::platform::x11::WindowAttributesExtX11::with_name(attributes, "flint", "flint")
    };
    match token {
        Some(token) => attributes.with_activation_token(ActivationToken::from_raw(token)),
        None => attributes,
    }
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
fn platform_attributes(attributes: WindowAttributes, _token: Option<String>) -> WindowAttributes {
    attributes
}
