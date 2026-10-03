//! Persistent UI state: everything that must survive between frames.
//!
//! Immediate mode rebuilds the whole tree every frame, so widget values live
//! here keyed by the element's `id`. Nothing in here is garbage-collected
//! automatically; call [`UiState::prune`] (or [`UiState::clear`]) to reclaim
//! space.

use std::collections::HashMap;

/// A value a widget owns between frames.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Num(f32),
    Text(String),
    /// A link was activated this frame; cleared after it is read.
    Link(String),
}

/// Per-element state, keyed by `id`.
#[derive(Clone, Debug, Default)]
pub struct UiState {
    values: HashMap<String, Value>,
    /// Ids seen this frame, used by `prune`.
    live: Vec<String>,
    /// Links activated during the current frame.
    pending_links: Vec<String>,
    hover_links: Vec<String>,
    /// Keyframe animations in flight, keyed by `(element id, animation name)`.
    ///
    /// The tree is rebuilt every frame, so this is where an animation's clock
    /// lives; without it every frame would restart it from zero.
    pub(crate) running: Vec<(String, crate::anim::Animation)>,
    /// Where each element was last frame, keyed by `id`.
    ///
    /// The tree is rebuilt from scratch, so hit-testing and parent sizing need
    /// the previous frame's rectangles to make sense.
    pub(crate) rects: HashMap<String, crate::geom::Rect>,
    /// Each element's resolved style last frame, keyed by `id`.
    ///
    /// Transitions interpolate from here, so it has to outlive the node it was
    /// computed on.
    pub(crate) prev_styles: HashMap<String, crate::style::Style>,
    /// In-flight transitions, keyed by `id`.
    pub(crate) tween_state:
        HashMap<String, Vec<(crate::anim::AnimProp, crate::ctx::Tween)>>,
    /// Global flags set by the app.
    pub flags: HashMap<String, bool>,
    /// Focus order for keyboard navigation, rebuilt each frame.
    pub focus_order: Vec<String>,
    /// The `id` of the element that has keyboard focus, if any.
    pub focus: Option<String>,
    /// Set when the user asked to close the window.
    pub quit: bool,
}

impl UiState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads a value, marking `id` as live.
    pub fn get(&mut self, id: &str) -> Option<&Value> {
        if !self.live.iter().any(|s| s == id) {
            self.live.push(id.to_string());
        }
        self.values.get(id)
    }

    pub fn get_bool(&mut self, id: &str, default: bool) -> bool {
        match self.get(id) {
            Some(Value::Bool(b)) => *b,
            _ => default,
        }
    }

    pub fn get_num(&mut self, id: &str, default: f32) -> f32 {
        match self.get(id) {
            Some(Value::Num(n)) => *n,
            _ => default,
        }
    }

    pub fn get_text(&mut self, id: &str, default: &str) -> String {
        match self.get(id) {
            Some(Value::Text(s)) => s.clone(),
            _ => default.to_string(),
        }
    }

    /// Stores a value and marks `id` as live, so `prune` keeps it.
    pub fn set(&mut self, id: &str, v: Value) {
        self.live.push(id.to_string());
        self.values.insert(id.to_string(), v);
    }

    /// Reads a value without marking `id` as live.
    ///
    /// Use this for ids that only need to survive until `prune` is not going to
    /// be called, such as values read outside a frame.
    pub fn peek(&self, id: &str) -> Option<&Value> {
        self.values.get(id)
    }

    pub fn set_bool(&mut self, id: &str, v: bool) {
        self.set(id, Value::Bool(v));
    }

    pub fn set_num(&mut self, id: &str, v: f32) {
        self.set(id, Value::Num(v));
    }

    pub fn set_text(&mut self, id: &str, v: impl Into<String>) {
        self.set(id, Value::Text(v.into()));
    }

    /// The text a text input should show.
    pub fn text_input_value(&mut self, id: &str) -> Option<String> {
        match self.values.get(id) {
            Some(Value::Text(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// Records that a link was clicked. Read with [`UiState::take_links`].
    pub fn push_link(&mut self, link: &str) {
        self.pending_links.push(link.to_string());
    }

    /// Links clicked during the current frame.
    pub fn take_links(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_links)
    }

    /// Links the cursor is currently over.
    pub fn hover_links(&self) -> &[String] {
        &self.hover_links
    }

    pub fn set_hover_links(&mut self, links: Vec<String>) {
        self.hover_links = links;
    }

    /// True while a flag is set.
    pub fn flag(&self, name: &str) -> bool {
        self.flags.get(name).copied().unwrap_or(false)
    }

    pub fn set_flag(&mut self, name: &str, on: bool) {
        self.flags.insert(name.to_string(), on);
    }

    pub fn toggle_flag(&mut self, name: &str) -> bool {
        let v = !self.flag(name);
        self.set_flag(name, v);
        v
    }

    /// Drops values for ids that were not touched this frame.
    ///
    /// Call it once at the end of a frame; it keeps the map from growing
    /// forever when UI is conditional.
    pub fn prune(&mut self) {
        // Mark a set once so `retain` does not scan the whole live list per key.
        let live: std::collections::HashSet<&str> =
            self.live.iter().map(|s| s.as_str()).collect();
        self.values.retain(|k, _| live.contains(k.as_str()));
        self.live.clear();
    }

    /// Forgets everything, including flags.
    pub fn clear(&mut self) {
        self.values.clear();
        self.live.clear();
        self.flags.clear();
    }

    /// Number of stored values, for diagnostics and tests.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip() {
        let mut s = UiState::new();
        assert_eq!(s.get_bool("a", false), false);
        s.set_bool("a", true);
        assert_eq!(s.get_bool("a", false), true);
        s.set_num("b", 2.5);
        assert_eq!(s.get_num("b", 0.0), 2.5);
        s.set_text("c", "hi");
        assert_eq!(s.get_text("c", ""), "hi");
        assert_eq!(s.text_input_value("c").as_deref(), Some("hi"));
    }

    #[test]
    fn prune_drops_ids_a_later_frame_did_not_touch() {
        let mut s = UiState::new();

        // Frame 1: both widgets are on screen.
        s.set_bool("keep", true);
        s.set_bool("drop", true);
        s.prune();
        assert!(s.peek("keep").is_some() && s.peek("drop").is_some());

        // Frame 2: only `keep` is rendered, so `drop` is no longer live.
        s.get_bool("keep", false);
        s.prune();
        assert!(s.peek("keep").is_some(), "a widget still on screen must survive");
        assert!(s.peek("drop").is_none(), "a removed widget must be collected");
    }

    #[test]
    fn peek_does_not_mark_an_id_live() {
        let mut s = UiState::new();
        s.set_bool("a", true);
        s.prune();
        assert!(s.peek("a").is_some(), "peek reads without keeping");
        s.prune();
        assert!(s.peek("a").is_none());
    }

    #[test]
    fn links_are_taken_once() {
        let mut s = UiState::new();
        s.push_link("github.com");
        assert_eq!(s.take_links(), vec!["github.com".to_string()]);
        assert!(s.take_links().is_empty());
    }

    #[test]
    fn flags_toggle() {
        let mut s = UiState::new();
        assert!(!s.flag("dark"));
        assert!(s.toggle_flag("dark"));
        assert!(!s.toggle_flag("dark"));
    }
}