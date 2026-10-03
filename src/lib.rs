//! MiMUI — an immediate-mode GUI library for Rust.
//!
//! ```
//! use mimui::prelude::*;
//!
//! struct MyApp;
//! impl App for MyApp {
//!     fn ui(&mut self, cx: &mut UiCtx) {
//!         ui! { MiMUI {
//!             Label "MiMUI";
//!             Button "Click!" link="openoptionswindow";
//!             Button class="bottom-btn" "github" link="github.com";
//!         } }
//!     }
//! }
//! ```

pub mod anim;
pub mod color;
pub mod css;
pub mod easing;
pub mod geom;
pub mod image;
pub mod input;
pub mod layout;
pub mod state;
pub mod style;
pub mod text;
pub mod widget;

pub mod ctx;

pub use ctx::{
    Clock, DrawCmd, Frame, ImageContent, Node, TextContent, TextureRef, Transform, UiCtx,
};
pub use anim::{AnimProp, AnimVal};
pub use widget::Attr;
// Re-exported at the crate root so `use mimui::anim;` works next to `ui!`.
pub use mimui_macros::ui;

mod app;
mod renderer;

pub use app::{open_in_browser, run, App, WindowOptions};

/// Everything you normally need.
pub mod prelude {
    /// The `anim!` macro and the animation module share a name; importing the
    /// module also brings the macro, since macros live in their own namespace.
    pub use crate::anim;
    pub use crate::anim::{Animation, Iter, Keyframes};
    pub use crate::app::{open_in_browser, run, App, WindowOptions};
    pub use crate::color::Color;
    pub use crate::easing::Easing;
    pub use crate::geom::{Corners, Dim, Edges, Rect, Vec2};
    pub use crate::input::{Key, MouseButton};
    pub use crate::state::UiState;
    pub use crate::style::Style;
    pub use crate::ui;
    pub use crate::ctx::{Clock, DrawCmd, Frame, TextureRef, UiCtx};
}
