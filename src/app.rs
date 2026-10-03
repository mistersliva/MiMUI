//! The application shell: window, event loop and the frame cycle.

use crate::ctx::{Clock, UiCtx};
use crate::geom::{Rect, Vec2};
use crate::input::{InputState, Key, Mods, MouseButton};
use crate::state::UiState;
use crate::style::Style;
use crate::text::TextEngine;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton as WMouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::{Window, WindowId};

/// Window and startup configuration.
pub struct WindowOptions {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub min_width: u32,
    pub min_height: u32,
    /// Base style every element inherits.
    pub theme: Style,
    /// Rules registered with `cx.add_class`, applied as a stylesheet.
    pub stylesheet: String,
    /// Extra font files to load, as `(bytes, family)` pairs.
    pub fonts: Vec<(Vec<u8>, String)>,
    /// Target frames per second. `0` draws only when something changes.
    pub fps: u32,
    pub resizable: bool,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: "MiMUI".to_string(),
            width: 960,
            height: 640,
            min_width: 320,
            min_height: 240,
            theme: Style::default(),
            stylesheet: String::new(),
            fonts: Vec::new(),
            fps: 0,
            resizable: true,
        }
    }
}

impl WindowOptions {
    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }
    pub fn size(mut self, w: u32, h: u32) -> Self {
        self.width = w;
        self.height = h;
        self
    }
    pub fn stylesheet(mut self, s: impl Into<String>) -> Self {
        self.stylesheet = s.into();
        self
    }
    pub fn theme(mut self, t: Style) -> Self {
        self.theme = t;
        self
    }
    /// Loads a font file (TTF/OTF bytes) and makes it the default family.
    pub fn font(mut self, data: Vec<u8>, family: impl Into<String>) -> Self {
        self.fonts.push((data, family.into()));
        self
    }
    /// Continuously redraw at this rate instead of on demand.
    pub fn fps(mut self, fps: u32) -> Self {
        self.fps = fps;
        self
    }
    pub fn min_size(mut self, w: u32, h: u32) -> Self {
        self.min_width = w;
        self.min_height = h;
        self
    }
}

/// Your application.
///
/// Everything is rebuilt every frame, so keep state in `self`.
pub trait App {
    /// Builds the UI. The `ui!` macro reads `cx` from this scope.
    fn ui(&mut self, cx: &mut UiCtx<'_>);

    /// Called before `ui` each frame, with the seconds since the last frame.
    fn update(&mut self, _dt: f32) {}

    /// Called with every link activated by a `Button ... link="..."`.
    ///
    /// The default implementation opens `http(s)://` URLs in the browser and
    /// ignores everything else, so app-specific links are usually handled by
    /// overriding this.
    fn on_link(&mut self, link: &str) {
        if link.starts_with("http://") || link.starts_with("https://") {
            open_in_browser(link);
        }
    }
}

/// Opens a URL in the user's default browser.
pub fn open_in_browser(url: &str) {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    let _ = cmd.spawn();
}

/// Runs an app until the window closes.
pub fn run<A: App + 'static>(app: A, options: WindowOptions) {
    let event_loop = EventLoop::new().expect("failed to create the event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut shell = Shell::new(app, options);
    event_loop.run_app(&mut shell).expect("event loop failed");
}

/// The winit handler that owns the window, GPU state and UI resources.
struct Shell<A: App> {
    app: A,
    options: WindowOptions,
    window: Option<std::sync::Arc<Window>>,
    renderer: Option<crate::renderer::Renderer>,
    input: InputState,
    state: UiState,
    text: TextEngine,
    clock: Clock,
    start: Instant,
    size: (u32, u32),
    scale: f32,
    redraw: bool,
    quit: bool,
}

impl<A: App> Shell<A> {
    fn new(app: A, options: WindowOptions) -> Self {
        Self {
            app,
            options,
            window: None,
            renderer: None,
            input: InputState::new(),
            state: UiState::new(),
            text: TextEngine::new(),
            clock: Clock::default(),
            start: Instant::now(),
            size: (960, 640),
            scale: 1.0,
            redraw: true,
            quit: false,
        }
    }

    /// Runs one frame: update, build the UI, lay it out, render.
    fn draw_frame(&mut self) {
        let Some(window) = self.window.clone() else { return };
        if self.renderer.is_none() {
            return;
        }

        // Advance the clock.
        let now = self.start.elapsed().as_secs_f32();
        let dt = (now - self.clock.now).clamp(0.0, 0.1);
        self.clock.now = now;
        self.clock.dt = dt;
        self.clock.frame += 1;

        self.app.update(dt);

        let size = window.inner_size();
        self.size = (size.width, size.height);
        // The UI is laid out in logical pixels; the renderer scales back up.
        let s = self.scale.max(0.01);
        let viewport = Vec2::new(size.width as f32 / s, size.height as f32 / s);

        // Split the borrows so the context can hold several at once.
        let Self { app, input, state, text, clock, options, scale, redraw, .. } = self;
        let theme = options.theme.clone();
        let stylesheet = options.stylesheet.clone();
        let scale = *scale;

        let frame = {
            let mut cx = UiCtx::new(input, state, text, clock, viewport, scale);
            cx.set_theme(theme);
            cx.add_stylesheet(&stylesheet);

            app.ui(&mut cx);

            cx.resolve_styles(dt);
            cx.measure_text();
            cx.layout(Rect::new(0.0, 0.0, viewport.x, viewport.y));
            cx.handle_interactions();

            let frame = cx.build_frame();
            *redraw |= frame.needs_redraw;
            frame
        };

        // Links activated this frame.
        let links = self.state.take_links();
        if !links.is_empty() {
            for link in &links {
                self.app.on_link(link);
            }
            self.redraw = true;
        }

        // Hand the renderer anything the frame needs uploaded.
        if frame.dirty_text_atlas {
            let size = self.text.atlas.size;
            let pixels = std::mem::take(&mut self.text.atlas.pixels);
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.upload_glyph_atlas(size, &pixels);
            }
            self.text.atlas.pixels = pixels;
            self.redraw = true;
        }
        if let Some(renderer) = self.renderer.as_mut() {
            for img in &frame.missing {
                renderer.ensure_image(img);
            }
            renderer.render(&frame, viewport, self.scale);
        }
        window.pre_present_notify();

        self.state.prune();
        self.input.end_frame();
    }
}

impl<A: App> ApplicationHandler for Shell<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title(&self.options.title)
            .with_inner_size(PhysicalSize::new(self.options.width, self.options.height))
            .with_min_inner_size(PhysicalSize::new(self.options.min_width, self.options.min_height))
            .with_resizable(self.options.resizable);

        let window = match event_loop.create_window(attrs) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("MiMUI: could not create a window: {e}");
                self.quit = true;
                event_loop.exit();
                return;
            }
        };

        let size = window.inner_size();
        self.size = (size.width, size.height);
        self.scale = window.scale_factor() as f32;

        // The renderer takes ownership of an `Arc<Window>` so the surface can
        // borrow the handle for `'static`.
        let window = std::sync::Arc::new(window);
        let renderer = match crate::renderer::Renderer::new(window.clone()) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("MiMUI: could not initialise the renderer: {e}");
                self.quit = true;
                event_loop.exit();
                return;
            }
        };

        // Load the requested fonts.
        let fonts = std::mem::take(&mut self.options.fonts);
        for (data, family) in fonts {
            self.text.set_font(&data, &family);
        }

        self.window = Some(window);
        self.renderer = Some(renderer);
        self.redraw = true;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.quit = true;
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                self.size = (size.width, size.height);
                // Keep the logical size for hit-testing in step with the new
                // surface, and pick up a scale factor that changed with it.
                if let Some(w) = self.window.as_ref() {
                    self.scale = w.scale_factor() as f32;
                }
                self.redraw = true;
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale = scale_factor as f32;
                self.redraw = true;
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.input.mouse = Vec2::new(position.x as f32, position.y as f32);
                self.input.mouse_moved_this_frame = true;
                self.redraw = true;
            }
            WindowEvent::CursorLeft { .. } => {
                self.input.mouse = Vec2::splat(-1.0e4);
                self.redraw = true;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = to_button(button);
                match state {
                    ElementState::Pressed => self.input.press_button(b),
                    ElementState::Released => self.input.release_button(b),
                }
                self.redraw = true;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let d = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(x as f32, y as f32),
                    MouseScrollDelta::PixelDelta(p) => {
                        Vec2::new(p.x as f32, p.y as f32) * 0.05
                    }
                };
                self.input.scroll_delta += d;
                self.redraw = true;
            }
            WindowEvent::ModifiersChanged(m) => {
                // Modifier state arrives separately from key events.
                let m = m.state();
                self.input.mods = Mods {
                    shift: m.shift_key(),
                    ctrl: m.control_key(),
                    alt: m.alt_key(),
                    logo: m.super_key(),
                };
                self.redraw = true;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    if let Some(k) = to_key(&event.logical_key) {
                        self.input.press_key(k);
                    }
                    // Only plain typing inserts text, so shortcuts do not.
                    if let Some(t) = &event.text
                        && !self.input.mods.command()
                        && !self.input.mods.alt
                    {
                        self.input.text_delta.push_str(t);
                    }
                } else if let Some(k) = to_key(&event.logical_key) {
                    self.input.release_key(k);
                }
                self.redraw = true;
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.quit {
            event_loop.exit();
            return;
        }

        if self.options.fps > 0 {
            // Continuous mode: always schedule the next frame.
            self.redraw = true;
            event_loop.set_control_flow(ControlFlow::Poll);
        }

        if self.redraw {
            // Clear first: `draw_frame` may raise the flag again for an
            // animation, and that request has to survive into the next wait.
            self.redraw = false;
            self.draw_frame();
        }
    }
}

fn to_button(b: WMouseButton) -> MouseButton {
    match b {
        WMouseButton::Left => MouseButton::Left,
        WMouseButton::Right => MouseButton::Right,
        WMouseButton::Middle => MouseButton::Middle,
        WMouseButton::Back => MouseButton::Other(3),
        WMouseButton::Forward => MouseButton::Other(4),
        WMouseButton::Other(n) => MouseButton::Other(n as u16),
    }
}

fn to_key(k: &WKey) -> Option<Key> {
    Some(match k {
        WKey::Named(NamedKey::Enter) => Key::Enter,
        WKey::Named(NamedKey::Escape) => Key::Escape,
        WKey::Named(NamedKey::Tab) => Key::Tab,
        WKey::Named(NamedKey::Backspace) => Key::Backspace,
        WKey::Named(NamedKey::Delete) => Key::Delete,
        WKey::Named(NamedKey::ArrowLeft) => Key::Left,
        WKey::Named(NamedKey::ArrowRight) => Key::Right,
        WKey::Named(NamedKey::ArrowUp) => Key::Up,
        WKey::Named(NamedKey::ArrowDown) => Key::Down,
        WKey::Named(NamedKey::Home) => Key::Home,
        WKey::Named(NamedKey::End) => Key::End,
        WKey::Named(NamedKey::PageUp) => Key::PageUp,
        WKey::Named(NamedKey::PageDown) => Key::PageDown,
        WKey::Named(NamedKey::Space) => Key::Space,
        WKey::Named(NamedKey::Shift) => Key::Shift,
        WKey::Named(NamedKey::Control) => Key::Ctrl,
        WKey::Named(NamedKey::Alt) => Key::Alt,
        // winit's `Key::Character` holds a `SmolStr`, so take the first char.
        WKey::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            if c.is_control() {
                return None;
            }
            Key::Char(c)
        }
        _ => return None,
    })
}