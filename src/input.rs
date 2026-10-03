//! Input state: mouse, keyboard, focus and scrolling.

use crate::geom::Vec2;

/// A mouse or touch button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u16),
}

impl MouseButton {
    pub fn is_left(self) -> bool {
        self == MouseButton::Left
    }
}

/// A keyboard key, kept as a small enum plus a raw code so bindings work
/// without pulling in a full input-capture crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Space,
    Shift,
    Ctrl,
    Alt,
    /// Anything we do not name; carries the platform code.
    Other(u32),
}

impl Key {
    /// True for keys that produce a character when typing.
    pub fn is_printable(self) -> bool {
        matches!(self, Key::Char(c) if !c.is_control())
    }
}

/// A keyboard modifier set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

impl Mods {
    pub const NONE: Self = Self { shift: false, ctrl: false, alt: false, logo: false };

    pub fn any(self) -> bool {
        self.shift || self.ctrl || self.alt || self.logo
    }
    /// The platform's "command" modifier.
    pub fn command(self) -> bool {
        self.logo || self.ctrl
    }
}

/// A handle to one element in the UI tree.
///
/// Ids are assigned in tree order every frame. Persistence across frames is
/// handled by matching an explicit `id` string, not by holding an `UiId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UiId(pub usize);

impl UiId {
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Persistent input state, updated from window events.
///
/// `hot` and `active` hold node indices into the current frame's tree. Keyboard
/// focus lives in [`crate::state::UiState`] instead, keyed by element `id`,
/// because the tree is rebuilt every frame and indices do not survive.
#[derive(Clone, Debug, Default)]
pub struct InputState {
    /// Where the mouse is, in logical pixels.
    pub mouse: Vec2,
    pub mouse_down: Vec<MouseButton>,
    pub mouse_moved_this_frame: bool,
    pub scroll_delta: Vec2,
    /// Widget under the cursor this frame.
    pub hot: Option<usize>,
    /// Widget that captured the pointer and will receive the release event.
    pub active: Option<usize>,
    pub pressed_keys: Vec<Key>,
    pub mods: Mods,
    pub text_delta: String,

    // Per-frame edge sets, cleared by `end_frame`.
    pressed_this_frame: Vec<MouseButton>,
    released_this_frame: Vec<MouseButton>,
    keys_pressed_this_frame: Vec<Key>,
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_down(&self, b: MouseButton) -> bool {
        self.mouse_down.contains(&b)
    }

    /// True on the frame a button went down.
    pub fn just_pressed(&self, b: MouseButton) -> bool {
        self.pressed_this_frame.contains(&b)
    }

    /// True on the frame a button went up.
    pub fn just_released(&self, b: MouseButton) -> bool {
        self.released_this_frame.contains(&b)
    }

    pub fn is_shift(&self) -> bool {
        self.mods.shift
    }
    pub fn is_ctrl(&self) -> bool {
        self.mods.ctrl
    }
    pub fn is_alt(&self) -> bool {
        self.mods.alt
    }
    /// Command on macOS, Control elsewhere.
    pub fn is_command(&self) -> bool {
        self.mods.command()
    }

    pub fn pressed(&self, k: Key) -> bool {
        self.pressed_keys.contains(&k)
    }

    /// Clears the per-frame flags. Called at the end of each frame.
    pub fn end_frame(&mut self) {
        self.mouse_moved_this_frame = false;
        self.scroll_delta = Vec2::ZERO;
        self.text_delta.clear();
        self.pressed_this_frame.clear();
        self.released_this_frame.clear();
        self.keys_pressed_this_frame.clear();
    }
}

// The per-frame edge sets live next to the state they describe.
impl InputState {
    pub fn press_button(&mut self, b: MouseButton) {
        if !self.mouse_down.contains(&b) {
            self.mouse_down.push(b);
            self.pressed_this_frame.push(b);
        }
    }

    pub fn release_button(&mut self, b: MouseButton) {
        self.mouse_down.retain(|&x| x != b);
        if !self.released_this_frame.contains(&b) {
            self.released_this_frame.push(b);
        }
    }

    pub fn press_key(&mut self, k: Key) {
        if !self.pressed_keys.contains(&k) {
            self.pressed_keys.push(k);
            self.keys_pressed_this_frame.push(k);
        }
    }

    pub fn release_key(&mut self, k: Key) {
        self.pressed_keys.retain(|&x| x != k);
    }
}