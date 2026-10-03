//! The UI context: builds the node tree, resolves styles, runs layout and
//! produces the draw list the renderer consumes.
//!
//! Frame lifecycle:
//!
//! ```text
//! elem/end …  ->  resolve_styles -> measure_text -> layout
//!             ->  handle_interactions -> build_frame -> render
//! ```

use crate::anim::{self, AnimProp, AnimVal, Animation};
use crate::color::Color;
use crate::geom::{Corners, Rect, Vec2};
use crate::image::{Fit, ImageKind};
use crate::input::{InputState, Key, MouseButton, UiId};
use crate::layout::{self, Item};
use crate::state::UiState;
use crate::style::{Align, CursorKind, Display, Filter, Style, TriggerSet, TriggerStyles};
use crate::text::{ShapedText, TextEngine};
use crate::widget::{InteractionKind, Resolved, WidgetSpec};
use std::collections::HashMap;

/// Text a node wants to draw.
#[derive(Clone, Debug, Default)]
pub struct TextContent {
    pub text: String,
    /// True when this is placeholder text rather than a real value.
    pub placeholder: bool,
    /// Filled in during measurement.
    pub shaped: ShapedText,
}

/// An image a node wants to draw.
#[derive(Clone, Debug)]
pub struct ImageContent {
    pub source: String,
    pub kind: ImageKind,
    pub fit: Fit,
    pub tint: Color,
    /// Natural pixel size, filled in when the image is decoded.
    pub intrinsic: Vec2,
}

/// A running transition for one property on one node.
#[derive(Clone, Debug)]
pub struct Tween {
    from: AnimVal,
    to: AnimVal,
    started: f32,
    duration: f32,
    delay: f32,
    ease: crate::easing::Easing,
}

/// One element of the UI tree.
pub struct Node {
    pub tag: String,
    pub id: String,
    pub parent: Option<usize>,
    pub children: Vec<usize>,

    /// Per-state styles as declared.
    pub styles: TriggerStyles,
    /// The style used this frame, with animations and transitions applied.
    pub resolved: Style,
    /// Last frame's resolved style, used to detect transition starts.
    pub previous: Option<Style>,

    pub text: Option<TextContent>,
    pub image: Option<ImageContent>,
    pub interaction: InteractionKind,
    /// Whether the node's size comes from its text.
    pub measure_text: bool,

    pub states: TriggerSet,
    pub disabled: bool,
    pub animations: Vec<Animation>,
    pub tweens: Vec<(AnimProp, Tween)>,

    /// Final position and size.
    pub rect: Rect,
    /// Size the node wants, including margins.
    pub content: Vec2,
    /// Extra size from animated `width`/`height`.
    pub anim_size: Vec2,
    pub clip: Option<Rect>,
    /// Scroll offset applied to children.
    pub scroll: Vec2,

    pub hovered: bool,
    pub clicked: bool,
}

impl Node {
    fn new(tag: &str, id: &str) -> Self {
        Self {
            tag: tag.to_string(),
            id: id.to_string(),
            parent: None,
            children: Vec::new(),
            styles: TriggerStyles::default(),
            resolved: Style::default(),
            previous: None,
            text: None,
            image: None,
            interaction: InteractionKind::None,
            measure_text: false,
            states: TriggerSet::default(),
            disabled: false,
            animations: Vec::new(),
            tweens: Vec::new(),
            rect: Rect::ZERO,
            content: Vec2::ZERO,
            anim_size: Vec2::ZERO,
            clip: None,
            scroll: Vec2::ZERO,
            hovered: false,
            clicked: false,
        }
    }

    pub fn style(&self) -> &Style {
        &self.resolved
    }
}

/// Which texture a draw command samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureRef {
    GlyphAtlas,
    /// A hashed image.
    Image(u64),
}

/// A 2D affine transform applied to a draw command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub translate: Vec2,
    pub scale: Vec2,
    pub rotate: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self { translate: Vec2::ZERO, scale: Vec2::ONE, rotate: 0.0 }
    }
}

impl Transform {
    pub fn is_identity(self) -> bool {
        self == Transform::default()
    }

    /// The four corners of a rect after this transform.
    pub fn corners(self, r: Rect) -> [Vec2; 4] {
        let c = r.center();
        let (sin, cos) = self.rotate.sin_cos();
        let pts = [
            Vec2::new(r.x, r.y),
            Vec2::new(r.x + r.w, r.y),
            Vec2::new(r.x + r.w, r.y + r.h),
            Vec2::new(r.x, r.y + r.h),
        ];
        let mut out = [Vec2::ZERO; 4];
        for (i, p) in pts.iter().enumerate() {
            let d = Vec2::new((p.x - c.x) * self.scale.x, (p.y - c.y) * self.scale.y);
            out[i] = Vec2::new(
                c.x + d.x * cos - d.y * sin + self.translate.x,
                c.y + d.x * sin + d.y * cos + self.translate.y,
            );
        }
        out
    }

    /// The axis-aligned bounds of a transformed rect.
    pub fn aabb(self, r: Rect) -> Rect {
        if self.is_identity() {
            return r;
        }
        let c = self.corners(r);
        let mut out = Rect::new(c[0].x, c[0].y, 0.0, 0.0);
        out.union_point(c[1]);
        out.union_point(c[2]);
        out.union_point(c[3]);
        out
    }
}

/// A primitive the renderer can draw.
#[derive(Clone, Debug)]
pub enum DrawCmd {
    /// A rounded box with an optional border and shadow.
    Box {
        rect: Rect,
        radius: Corners<f32>,
        fill: Option<Color>,
        /// `(width, color)`
        border: Option<(f32, Color)>,
        shadow: Option<crate::style::BoxShadow>,
        clip: Option<Rect>,
        opacity: f32,
        filter: Filter,
        transform: Transform,
    },
    /// A textured quad: an image or a glyph from the atlas.
    Image {
        rect: Rect,
        uv: Rect,
        tint: Color,
        texture: TextureRef,
        clip: Option<Rect>,
        opacity: f32,
        radius: Corners<f32>,
        filter: Filter,
        transform: Transform,
    },
}

/// Everything the renderer needs for one frame.
pub struct Frame {
    pub cmds: Vec<DrawCmd>,
    pub clear: Color,
    /// Images referenced this frame that still need decoding and upload.
    pub missing: Vec<ImageContent>,
    pub cursor: CursorKind,
    pub dirty_text_atlas: bool,
    pub needs_redraw: bool,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            cmds: Vec::new(),
            clear: Color::rgb(16, 19, 28, 1.0),
            missing: Vec::new(),
            cursor: CursorKind::Default,
            dirty_text_atlas: false,
            needs_redraw: false,
        }
    }
}

/// Frame timing.
pub struct Clock {
    pub now: f32,
    pub dt: f32,
    pub frame: u64,
}

impl Default for Clock {
    fn default() -> Self {
        Self { now: 0.0, dt: 0.0, frame: 0 }
    }
}

/// Builds the UI for one frame. The `ui!` macro drives this.
pub struct UiCtx<'a> {
    pub(crate) nodes: Vec<Node>,
    /// Parent for the next node pushed.
    pub(crate) current: Option<usize>,
    /// Nesting so `end` can restore the parent.
    pub(crate) stack: Vec<Option<usize>>,
    /// Extra nodes pushed by widgets (button internals, slider parts).

    pub(crate) input: &'a mut InputState,
    pub(crate) state: &'a mut UiState,
    pub(crate) text: &'a mut TextEngine,
    pub(crate) clock: &'a Clock,
    pub(crate) viewport: Vec2,
    pub(crate) scale: f32,

    pub(crate) css: HashMap<String, String>,
    pub(crate) theme: Style,
    pub(crate) disabled_depth: usize,
    /// `(node id, link target)` for buttons that navigate.
    pub(crate) links: Vec<(usize, String)>,

    pub(crate) counter: usize,
    pub(crate) cursor: CursorKind,
    pub(crate) needs_redraw: bool,
    /// Set once per frame when the pointer is over a button that navigates.
    pub(crate) link_hover: Option<String>,
}

impl<'a> UiCtx<'a> {
    /// Creates a context for one frame.
    pub fn new(
        input: &'a mut InputState,
        state: &'a mut UiState,
        text: &'a mut TextEngine,
        clock: &'a Clock,
        viewport: Vec2,
        scale: f32,
    ) -> Self {
        Self {
            nodes: Vec::new(),
            current: None,
            stack: Vec::new(),
            input,
            state,
            text,
            clock,
            viewport,
            scale,
            css: HashMap::new(),
            theme: Style::default(),
            disabled_depth: 0,
            links: Vec::new(),
            counter: 0,
            cursor: CursorKind::Default,
            needs_redraw: false,
            link_hover: None,
        }
    }

    /// Runs `f` with this context. Used by the `ui!` macro.
    pub fn with<R>(cx: &mut UiCtx<'a>, f: impl FnOnce(&mut UiCtx<'a>) -> R) -> R {
        f(cx)
    }

    // -- configuration ------------------------------------------------------

    /// Registers a class rule; later rules win.
    pub fn add_class(&mut self, name: &str, decls: &str) {
        self.css.insert(name.to_ascii_lowercase(), decls.to_string());
    }

    pub fn css_for(&self, name: &str) -> Option<&str> {
        self.css.get(&name.to_ascii_lowercase()).map(|s| s.as_str())
    }

    /// Loads a stylesheet: `.card { padding: 8px; }`, `#title { color: red; }`.
    pub fn add_stylesheet(&mut self, src: &str) {
        for (selector, body) in parse_stylesheet(src) {
            if let Some(name) = selector.strip_prefix('.') {
                self.add_class(name, &body);
            } else if let Some(id) = selector.strip_prefix('#') {
                self.add_class(&format!("#{id}"), &body);
            }
        }
    }

    pub fn set_theme(&mut self, style: Style) {
        self.theme = style;
    }

    pub fn theme(&self) -> &Style {
        &self.theme
    }

    pub fn viewport(&self) -> Vec2 {
        self.viewport
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn dt(&self) -> f32 {
        self.clock.dt
    }

    pub fn now(&self) -> f32 {
        self.clock.now
    }

    pub fn frame_index(&self) -> u64 {
        self.clock.frame
    }

    pub fn input(&self) -> &InputState {
        self.input
    }

    pub fn state(&mut self) -> &mut UiState {
        self.state
    }

    /// The text engine, for callers that load fonts or inspect the atlas.
    pub fn text(&self) -> &TextEngine {
        self.text
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn node(&self, i: usize) -> Option<&Node> {
        self.nodes.get(i)
    }

    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    pub fn request_redraw(&mut self) {
        self.needs_redraw = true;
    }

    // -- macro entry points --------------------------------------------------

    /// Opens an element. Called by the `ui!` macro.
    ///
    /// The macro nesting is tracked separately from the node tree: this
    /// remembers the parent so the matching `end` can restore it, while widgets
    /// use [`UiCtx::push_node`] / [`UiCtx::close_node`] for their own children.
    pub fn elem(
        &mut self,
        tag: &str,
        attrs: Vec<(&'static str, crate::widget::Attr)>,
        is_root: bool,
    ) {
        self.stack.push(self.current);

        let norm = crate::widget::normalize_tag(tag);
        let text = attrs
            .iter()
            .find(|(k, _)| *k == "text")
            .and_then(|(_, v)| v.as_str())
            .map(str::to_string);
        let spec = WidgetSpec { tag: &norm, attrs: &attrs[..], text: text.as_deref() };

        if is_root {
            // The root inherits the window theme and stacks like a document, so
            // its children fill the window's width. Write `direction="row"` on
            // `MiMUI` for a horizontal root.
            let theme = self.theme.clone();
            let mut root = Resolved::from_spec(self, &spec);
            root.styles.base = crate::style::merge_partial(&theme, &root.styles.base);
            let mut defaults = Style::default();
            defaults.direction = crate::style::Direction::Column;
            root.merge_defaults(&defaults);
            let id = self.fresh_id();
            let styles = root.styles.clone();
            let n = self.push_node(&id, &norm, styles);
            root.start_animations(self, n);
        } else {
            let resolved = Resolved::from_spec(self, &spec);
            crate::widget::dispatch(self, &norm, &spec, resolved);
        }
    }

    /// Closes the element opened by [`UiCtx::elem`].
    pub fn end(&mut self) {
        self.current = self.stack.pop().flatten();
    }

    fn fresh_id(&mut self) -> String {
        self.counter += 1;
        format!("n{}", self.counter)
    }

    /// The next auto-generated id for a widget tag.
    pub(crate) fn next_id(&mut self, tag: &str) -> String {
        self.counter += 1;
        format!("{}#{}", tag, self.counter)
    }

    // -- node construction ---------------------------------------------------

    /// Pushes a node; later nodes become its children until it is closed.
    ///
    /// Nesting is recovered from [`Node::parent`], so this never touches the
    /// macro stack that [`UiCtx::elem`] and [`UiCtx::end`] use.
    pub fn push_node(&mut self, id: &str, tag: &str, styles: TriggerStyles) -> usize {
        let index = self.nodes.len();
        let mut node = Node::new(tag, id);
        node.parent = self.current;
        node.styles = styles;
        // Start from where this element was last frame so hover states and
        // parent sizing have something to work with before layout runs.
        if let Some(prev) = self.state.rects.get(id) {
            node.rect = *prev;
        }
        node.previous = self.state.prev_styles.get(id).cloned();
        node.tweens = self.state.tween_state.get(id).cloned().unwrap_or_default();
        self.nodes.push(node);
        if let Some(p) = self.current {
            self.nodes[p].children.push(index);
        }
        self.current = Some(index);
        index
    }

    /// Makes `index`'s parent the current node, closing that subtree.
    ///
    /// Call this after building a child subtree that should not stay open — for
    /// instance the tick inside a checkbox box, so the label that follows
    /// becomes a sibling of the box rather than a child of the tick.
    pub fn close_node(&mut self, index: usize) {
        if let Some(parent) = self.nodes.get(index).and_then(|n| n.parent) {
            self.current = Some(parent);
        }
    }

    pub fn push_disabled(&mut self) {
        self.disabled_depth += 1;
    }

    pub fn pop_disabled(&mut self) {
        self.disabled_depth = self.disabled_depth.saturating_sub(1);
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled_depth > 0
    }

    /// Declares a keyframe animation on a node.
    ///
    /// The declaration is repeated every frame, so an animation that is already
    /// in flight keeps its clock. Only a changed declaration restarts it, which
    /// keeps a finished `once` animation parked on its last frame instead of
    /// looping forever.
    pub fn push_animation(&mut self, node: usize, a: Animation) {
        let id = self.nodes[node].id.clone();
        let spec = a.spec.clone();
        let running = &mut self.state.running;
        match running.iter_mut().find(|(rid, r)| *rid == id && r.spec.name == spec.name) {
            Some((_, running)) => {
                if running.spec != spec {
                    *running = a;
                }
            }
            None => running.push((id, a)),
        }
        self.needs_redraw = true;
    }

    /// Notes that a button navigates to `link`.
    pub fn register_link(&mut self, node: usize, link: &str) {
        self.links.push((node, link.to_string()));
    }

    // -- setters used by widgets --------------------------------------------

    pub fn set_text(&mut self, node: usize, text: String, placeholder: bool) {
        self.nodes[node].text = Some(TextContent { text, placeholder, ..Default::default() });
    }

    pub fn set_image(&mut self, node: usize, source: String, kind: ImageKind, fit: Fit, tint: Color) {
        self.nodes[node].image = Some(ImageContent {
            source,
            kind,
            fit,
            tint,
            intrinsic: Vec2::ZERO,
        });
    }

    /// Records a decoded image's natural size so `fit` can work.
    pub fn set_image_size(&mut self, node: usize, size: Vec2) {
        if let Some(img) = self.nodes[node].image.as_mut() {
            img.intrinsic = size;
        }
    }

    pub fn set_interaction(&mut self, node: usize, kind: InteractionKind) {
        self.nodes[node].interaction = kind;
    }

    pub fn set_measure_text(&mut self, node: usize, on: bool) {
        self.nodes[node].measure_text = on;
    }

    pub fn set_styles(&mut self, node: usize, styles: TriggerStyles) {
        self.nodes[node].styles = styles;
    }

    pub fn node_id(&self, node: usize) -> UiId {
        UiId(node)
    }

    /// Moves the cursor to the centre of the first node with `tag`.
    ///
    /// Useful in tests: set the pointer before `resolve_styles` so the hover
    /// rules apply on the same frame.
    pub fn hover_tag(&mut self, tag: &str) {
        if let Some(n) = self.nodes.iter().find(|n| n.tag == tag) {
            let c = n.rect.center();
            self.input.mouse = c;
        }
    }

    /// Moves the cursor to the centre of the node with `id`.
    pub fn hover_id(&mut self, id: &str) {
        if let Some(n) = self.nodes.iter().find(|n| n.id == id) {
            let c = n.rect.center();
            self.input.mouse = c;
        }
    }

    /// The node index of the first node whose tag matches.
    pub fn find_by_tag(&self, tag: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.tag == tag)
    }

    /// The node index whose `id` matches.
    pub fn find_by_id(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    // -- style resolution ---------------------------------------------------

    /// Picks each node's style for the current interaction state, starts
    /// transitions, then applies animations and tweens.
    pub fn resolve_styles(&mut self, dt: f32) {
        let now = self.clock.now;
        let mouse = self.input.mouse;

        for i in 0..self.nodes.len() {
            let kind = self.nodes[i].interaction;
            let rect = self.nodes[i].rect;
            // A plain `Box` still reacts to the pointer if it declared an
            // `on_hover` block, so the trigger set is not limited to widgets.
            let reacts = kind != InteractionKind::None || self.nodes[i].styles.hover.is_some();
            let hit = reacts && !rect.is_empty() && rect.contains(mouse);
            let focused = self.state.focus.as_deref() == Some(self.nodes[i].id.as_str());

            let states = TriggerSet {
                hover: hit,
                active: hit && self.input.is_down(MouseButton::Left),
                focus: focused,
                disabled: self.is_disabled(),
            };
            self.nodes[i].states = states;
            self.nodes[i].disabled = states.disabled;

            let styles = self.nodes[i].styles.clone();
            let base = states.pick(&styles).clone();

            if let Some(prev) = self.nodes[i].previous.clone() {
                self.start_transitions(i, &prev, &base, now);
            }

            let mut style = base;

            // Only the animations that belong to this node's id.
            let id = self.nodes[i].id.clone();
            let mut running = false;
            for (_, a) in self.state.running.iter_mut().filter(|(rid, _)| *rid == id) {
                if a.advance(dt) {
                    running = true;
                }
                for (prop, val) in a.sample() {
                    prop.write(&mut style, val);
                }
            }
            if running {
                self.needs_redraw = true;
            }

            // In-flight transitions win over animations.
            self.apply_tweens(i, &mut style, now);

            // Keep the resolved style so the next frame can diff against it.
            // `Style` is not `Copy`, so clone it into the slot.
            self.nodes[i].resolved = style.clone();
            self.nodes[i].previous = Some(style.clone());
            let id = self.nodes[i].id.clone();
            self.state.prev_styles.insert(id.clone(), style);
            self.state.tween_state.insert(id, self.nodes[i].tweens.clone());
        }
    }

    /// The transition spec declared on a node, copied out to avoid aliasing.
    fn transition_of(&self, i: usize) -> crate::anim::Transition {
        self.nodes[i].styles.base.transition.clone()
    }

    fn start_transitions(&mut self, i: usize, prev: &Style, base: &Style, now: f32) {
        let trans = self.transition_of(i);
        if !trans.is_active() {
            return;
        }
        for &prop in anim::ALL_PROPS {
            if !trans.covers(prop) {
                continue;
            }
            let from = prop.read(prev);
            let to = prop.read(base);
            if from == to {
                continue;
            }
            let already = self.nodes[i]
                .tweens
                .iter()
                .any(|(p, t)| *p == prop && t.to == to && t.duration > 0.0);
            if already {
                continue;
            }
            // Start from wherever the tween currently is.
            let current = self.nodes[i]
                .tweens
                .iter_mut()
                .find(|(p, _)| *p == prop)
                .map(|(_, t)| tween_value(t, now))
                .unwrap_or(from);

            self.nodes[i].tweens.retain(|(p, _)| *p != prop);
            self.nodes[i].tweens.push((
                prop,
                Tween {
                    from: current,
                    to,
                    started: now,
                    duration: trans.duration,
                    delay: trans.delay,
                    ease: trans.ease,
                },
            ));
            self.needs_redraw = true;
        }
    }

    fn apply_tweens(&mut self, i: usize, style: &mut Style, now: f32) {
        if self.nodes[i].tweens.is_empty() {
            return;
        }
        let mut finished = true;
        for idx in 0..self.nodes[i].tweens.len() {
            let (prop, tween) = self.nodes[i].tweens[idx].clone();
            let value = tween_value(&tween, now);
            prop.write(style, value);
            if tween_progress(&tween, now) < 1.0 {
                finished = false;
            }
        }
        if finished {
            self.nodes[i].tweens.clear();
        } else {
            self.needs_redraw = true;
        }
    }

    // -- measurement and layout --------------------------------------------

    /// Shapes all text nodes and records their intrinsic sizes.
    pub fn measure_text(&mut self) {
        for i in 0..self.nodes.len() {
            if !self.nodes[i].measure_text {
                continue;
            }
            let Some(tc) = self.nodes[i].text.clone() else { continue };
            let attrs = crate::widget::attrs_from_style(&self.nodes[i].resolved);
            let shaped = if tc.text.is_empty() {
                ShapedText::default()
            } else {
                self.text.shape(&tc.text, &attrs)
            };
            if let Some(t) = self.nodes[i].text.as_mut() {
                t.shaped = shaped;
            }
            let (dw, dh) = self.anim_size(i);
            self.nodes[i].anim_size = Vec2::new(dw, dh);
        }
    }

    /// Extra size contributed by animated `width`/`height` properties.
    fn anim_size(&self, i: usize) -> (f32, f32) {
        let mut w = 0.0f32;
        let mut h = 0.0f32;
        let now = self.clock.now;
        for (p, t) in &self.nodes[i].tweens {
            let v = tween_value(t, now).num(0.0);
            match p {
                AnimProp::W => w = w.max(v),
                AnimProp::H => h = h.max(v),
                _ => {}
            }
        }
        for (_, a) in self.state.running.iter().filter(|(rid, _)| *rid == self.nodes[i].id) {
            for (p, v) in a.sample() {
                match p {
                    AnimProp::W => w = w.max(v.num(0.0)),
                    AnimProp::H => h = h.max(v.num(0.0)),
                    _ => {}
                }
            }
        }
        (w, h)
    }

    /// Lays the tree out, writing every node's rect.
    pub fn layout(&mut self, viewport: Rect) {
        if self.nodes.is_empty() {
            return;
        }
        self.nodes[0].rect = viewport;

        // Sizes, bottom-up.
        for i in self.post_order() {
            if self.nodes[i].parent.is_none() {
                continue;
            }
            let parent = self.nodes[i].parent.expect("checked");
            let avail = self.content_box(parent).size();

            let own = self.text_size(i);
            let (dw, dh) = self.anim_size(i);
            let own = Vec2::new(own.x + dw, own.y + dh);

            // Post-order guarantees the children are already sized.
            let kids: Vec<Vec2> =
                self.nodes[i].children.iter().map(|&c| self.nodes[c].content).collect();
            let style = self.nodes[i].resolved.clone();
            let size = layout::measure(&style, own, &kids, avail);
            let margin = layout::margins(&style, avail);
            self.nodes[i].content = Vec2::new(size.x + margin.x, size.y + margin.y);
        }

        // Positions, per container.
        for p in 0..self.nodes.len() {
            let kids = self.nodes[p].children.clone();
            if kids.is_empty() {
                continue;
            }
            let style = self.nodes[p].resolved.clone();
            // `flex` insets by padding itself, so hand it the border box.
            let sc = self.nodes[p].scroll;
            let r = self.nodes[p].rect;
            let box_ = Rect { x: r.x + sc.x, y: r.y + sc.y, w: r.w, h: r.h };

            let mut items: Vec<Item<'_>> = Vec::with_capacity(kids.len());
            for &k in &kids {
                items.push(Item {
                    style: &self.nodes[k].resolved,
                    content: self.nodes[k].content,
                    out_rect: Rect::ZERO,
                });
            }
            layout::flex(&box_, &style, &mut items);
            // Copy the results out before touching `self` again.
            let rects: Vec<Rect> = items.iter().map(|i| i.out_rect).collect();
            for (rect, &k) in rects.into_iter().zip(kids.iter()) {
                self.nodes[k].rect = rect;
            }
        }

        // Remember every position so next frame can hit-test against it.
        self.state.rects = self
            .nodes
            .iter()
            .map(|n| (n.id.clone(), n.rect))
            .collect();

        // Clip bounds have to be known before painting *and* hit-testing.
        if !self.nodes.is_empty() {
            self.apply_clip(0, None);
        }
    }

    /// Pushes each container's clipping bounds down to its children.
    fn apply_clip(&mut self, i: usize, parent: Option<Rect>) {
        let own = self.nodes[i].resolved.clone();
        if own.display == Display::None || !own.visible {
            self.nodes[i].clip = None;
            return;
        }
        let clip = if layout::clips(&own) {
            let inner = self.content_box(i);
            Some(match parent {
                Some(p) => p.intersection(&inner),
                None => inner,
            })
        } else {
            parent
        };
        self.nodes[i].clip = clip;
        for k in self.nodes[i].children.clone() {
            self.apply_clip(k, clip);
        }
    }

    /// The padding-adjusted box children are placed inside.
    fn content_box(&self, i: usize) -> Rect {
        self.nodes[i]
            .rect
            .inset_edges(layout::resolve_edges(&self.nodes[i].resolved.padding, self.nodes[i].rect.size()))
    }

    /// The size a node's own text wants.
    fn text_size(&self, i: usize) -> Vec2 {
        match &self.nodes[i].text {
            Some(t) => Vec2::new(t.shaped.width, t.shaped.height),
            None => Vec2::ZERO,
        }
    }

    fn post_order(&self) -> Vec<usize> {
        fn walk(nodes: &[Node], i: usize, out: &mut Vec<usize>) {
            for &c in &nodes[i].children {
                walk(nodes, c, out);
            }
            out.push(i);
        }
        let mut out = Vec::with_capacity(self.nodes.len());
        if !self.nodes.is_empty() {
            walk(&self.nodes, 0, &mut out);
        }
        out
    }

    // -- interaction --------------------------------------------------------

    /// Hit-tests the tree and applies widget behaviour.
    pub fn handle_interactions(&mut self) {
        let mouse = self.input.mouse;
        let pressed = self.input.just_pressed(MouseButton::Left);
        let released = self.input.just_released(MouseButton::Left);

        for n in &mut self.nodes {
            n.hovered = false;
            n.clicked = false;
        }

        // Later nodes paint on top, so the last hit wins.
        let mut top_hit: Option<usize> = None;
        for i in 0..self.nodes.len() {
            let r = self.nodes[i].rect;
            if self.nodes[i].interaction != InteractionKind::None
                && !r.is_empty()
                && r.contains(mouse)
            {
                top_hit = Some(i);
            }
        }

        if let Some(i) = top_hit {
            self.nodes[i].hovered = true;
            self.cursor = self.nodes[i].resolved.cursor;
        }

        if pressed {
            // Press only captures the pointer. Widgets activate on release, so
            // dragging off a button cancels it.
            self.input.active = top_hit;
            if let Some(i) = top_hit {
                let id = self.nodes[i].id.clone();
                self.state.focus = Some(id);
                // A slider is the exception: it tracks the pointer as it moves.
                if self.nodes[i].interaction == InteractionKind::Slider {
                    self.activate(i, mouse);
                }
            }
        } else if released {
            let active = self.input.active.take();
            if let (Some(a), Some(i)) = (active, top_hit)
                && a == i
            {
                self.activate(i, mouse);
            }
        }

        // Keep a slider in step with a drag that is still in progress.
        if let Some(i) = self.input.active
            && self.input.is_down(MouseButton::Left)
            && self.nodes[i].interaction == InteractionKind::Slider
        {
            self.activate(i, mouse);
        }

        // Tab walks the interactive elements in tree order.
        if self.input.pressed(Key::Tab) {
            self.move_focus(self.input.is_shift());
            self.needs_redraw = true;
        }

        let focused = self.state.focus.clone().and_then(|id| self.find_by_id(&id));

        // Keyboard activation of the focused widget.
        if let Some(i) = focused
            && (self.input.pressed(Key::Enter) || self.input.pressed(Key::Space))
        {
            self.activate(i, mouse);
        }

        // Text entry into the focused input.
        if let Some(i) = focused
            && self.nodes[i].interaction == InteractionKind::TextInput
        {
            let id = self.nodes[i].id.clone();
            let mut value = self.state.get_text(&id, "");
            let before = value.clone();
            if !self.input.text_delta.is_empty() {
                value.push_str(&self.input.text_delta);
            }
            if self.input.pressed(Key::Backspace) {
                value.pop();
            }
            if value != before {
                self.state.set_text(&id, value);
                self.needs_redraw = true;
            }
        }

        // Record hovered links so the app can react to them.
        let hovered: Vec<String> = self
            .links
            .iter()
            .filter(|(n, _)| self.nodes[*n].hovered)
            .map(|(_, l)| l.clone())
            .collect();
        self.link_hover = hovered.first().cloned();
        self.state.set_hover_links(hovered);
    }

    /// Moves keyboard focus one step through the interactive elements.
    fn move_focus(&mut self, backwards: bool) {
        let ring: Vec<String> = self
            .nodes
            .iter()
            .filter(|n| n.interaction != InteractionKind::None)
            .map(|n| n.id.clone())
            .collect();
        if ring.is_empty() {
            return;
        }
        let at = self
            .state
            .focus
            .as_ref()
            .and_then(|id| ring.iter().position(|r| r == id));
        let next = match (at, backwards) {
            (None, _) => 0,
            (Some(i), false) => (i + 1) % ring.len(),
            (Some(0), true) => ring.len() - 1,
            (Some(i), true) => i - 1,
        };
        self.state.focus = Some(ring[next].clone());
        self.state.focus_order = ring;
    }

    fn activate(&mut self, i: usize, mouse: Vec2) {
        let kind = self.nodes[i].interaction;
        let id = self.nodes[i].id.clone();
        self.nodes[i].clicked = true;
        match kind {
            InteractionKind::Button => {
                if let Some((_, link)) = self.links.iter().find(|(n, _)| *n == i) {
                    let link = link.clone();
                    self.state.push_link(&link);
                }
                self.needs_redraw = true;
            }
            InteractionKind::Checkbox => {
                let cur = self.state.get_bool(&id, false);
                self.state.set_bool(&id, !cur);
                self.needs_redraw = true;
            }
            InteractionKind::Slider => {
                let r = self.nodes[i].rect;
                if r.w > 0.0 {
                    self.state.set_num(&id, ((mouse.x - r.x) / r.w).clamp(0.0, 1.0));
                    self.needs_redraw = true;
                }
            }
            InteractionKind::Radio | InteractionKind::TextInput | InteractionKind::None => {}
        }
    }

    // -- output -------------------------------------------------------------

    /// Produces the draw list.
    pub fn build_frame(&mut self) -> Frame {
        let mut frame = Frame {
            clear: self.theme.background.unwrap_or(Color::rgb(16, 19, 28, 1.0)),
            dirty_text_atlas: self.text.take_dirty(),
            needs_redraw: self.needs_redraw,
            ..Default::default()
        };

        let vp = Rect::new(0.0, 0.0, self.viewport.x, self.viewport.y);
        if !self.nodes.is_empty() {
            self.paint_node(0, vp, &mut frame);
        }

        for n in &self.nodes {
            if let Some(img) = &n.image
                && img.intrinsic.is_zero()
            {
                frame.missing.push(img.clone());
            }
        }
        frame.cursor = self.cursor;
        frame
    }

    /// Walks a subtree in paint order, emitting commands.
    ///
    /// Clip bounds were settled during layout, so this only has to skip the
    /// nodes that are not drawn at all.
    fn paint_node(&mut self, i: usize, viewport: Rect, frame: &mut Frame) {
        let own = self.nodes[i].resolved.clone();
        if own.display == Display::None || !own.visible || own.opacity <= 0.002 {
            return;
        }
        let clip = self.nodes[i].clip;
        self.emit(i, viewport, clip, frame);
        for k in self.nodes[i].children.clone() {
            self.paint_node(k, viewport, frame);
        }
    }

    /// Emits commands for one node.
    fn emit(&mut self, i: usize, viewport: Rect, clip: Option<Rect>, frame: &mut Frame) {
        let style = self.nodes[i].resolved.clone();
        let rect = self.nodes[i].rect;

        if style.display == Display::None || !style.visible || style.opacity <= 0.002 {
            return;
        }

        // Animated offsets.
        let (dx, dy, dw, dh) = self.anim_offsets(i);
        let rect = Rect { w: rect.w + dw, h: rect.h + dh, ..rect }.translate(Vec2::new(dx, dy));

        let transform = Transform {
            translate: style.translate,
            scale: style.scale,
            rotate: style.rotate,
        };
        let painted = transform.aabb(rect);

        // Cull anything entirely off-screen.
        let visible = match clip {
            Some(c) => painted.intersection(&c).intersects(&viewport),
            None => painted.intersects(&viewport),
        };
        if painted.is_empty() || !visible {
            return;
        }

        let opacity = style.opacity;
        let border = if style.border_width > 0.0 && !style.border_color.is_transparent() {
            Some((style.border_width, style.border_color))
        } else {
            None
        };
        let has_shadow = style.shadow.blur > 0.0 || style.shadow.offset.len() > 0.0;
        if style.background.is_some() || border.is_some() || has_shadow {
            frame.cmds.push(DrawCmd::Box {
                rect,
                radius: style.radius.clamp(rect.size()),
                fill: style.background,
                border,
                shadow: has_shadow.then_some(style.shadow),
                clip,
                opacity,
                filter: style.filter,
                transform,
            });
        }

        // Image.
        if let Some(img) = self.nodes[i].image.clone() {
            let dest = if img.intrinsic.x > 0.0 && img.intrinsic.y > 0.0 {
                fit_rect(rect, img.intrinsic, img.fit)
            } else {
                rect
            };
            frame.cmds.push(DrawCmd::Image {
                rect: dest,
                uv: Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
                tint: img.tint,
                texture: TextureRef::Image(image_key(&img)),
                clip,
                opacity,
                radius: style.radius.clamp(dest.size()),
                filter: style.filter,
                transform,
            });
        }

        // Text.
        if let Some(tc) = self.nodes[i].text.clone()
            && !tc.shaped.is_empty()
        {
            let color = if tc.placeholder { style.color.fade(0.45) } else { style.color };
            // Text lives in the padding box and follows `align-items`, so a
            // Button's label is centred inside its own padding.
            let inner = self.content_box(i);
            let origin_y = match style.align_items {
                Align::Center => inner.y + (inner.h - tc.shaped.height) * 0.5,
                Align::End => inner.y + inner.h - tc.shaped.height,
                _ => inner.y,
            };
            let origin = Vec2::new(inner.x, origin_y);
            for line in &tc.shaped.lines {
                for g in &line.glyphs {
                    let Some(entry) = self.text.entry(g.font_id, g.glyph_id, g.size) else {
                        continue;
                    };
                    if entry.tex_w <= 0.0 || entry.tex_h <= 0.0 {
                        continue;
                    }
                    let q = Rect {
                        x: origin.x + g.x + g.left,
                        y: origin.y + g.y + g.top,
                        w: entry.width,
                        h: entry.height,
                    };
                    if q.is_empty() {
                        continue;
                    }
                    frame.cmds.push(DrawCmd::Image {
                        rect: q,
                        uv: Rect {
                            x: entry.u0,
                            y: entry.v0,
                            w: entry.u1 - entry.u0,
                            h: entry.v1 - entry.v0,
                        },
                        tint: color,
                        texture: TextureRef::GlyphAtlas,
                        clip,
                        opacity,
                        radius: Corners::splat(0.0),
                        filter: Filter::None,
                        transform,
                    });
                }
            }
        }
    }

    /// Animated `x`, `y`, `width` and `height` offsets for a node.
    fn anim_offsets(&self, i: usize) -> (f32, f32, f32, f32) {
        let mut x = 0.0;
        let mut y = 0.0;
        let mut w = 0.0;
        let mut h = 0.0;
        let now = self.clock.now;
        for (p, t) in &self.nodes[i].tweens {
            let v = tween_value(t, now).num(0.0);
            match p {
                AnimProp::X => x = v,
                AnimProp::Y => y = v,
                AnimProp::W => w = v,
                AnimProp::H => h = v,
                _ => {}
            }
        }
        for (_, a) in self.state.running.iter().filter(|(rid, _)| *rid == self.nodes[i].id) {
            for (p, v) in a.sample() {
                match p {
                    AnimProp::X => x = v.num(0.0),
                    AnimProp::Y => y = v.num(0.0),
                    AnimProp::W => w = v.num(0.0),
                    AnimProp::H => h = v.num(0.0),
                    _ => {}
                }
            }
        }
        (x, y, w, h)
    }
}

fn tween_value(t: &Tween, now: f32) -> AnimVal {
    let k = crate::easing::apply(t.ease, tween_progress(t, now));
    t.from.lerp(t.to, k)
}

fn tween_progress(t: &Tween, now: f32) -> f32 {
    let elapsed = now - t.started - t.delay;
    if elapsed <= 0.0 {
        return 0.0;
    }
    if t.duration <= 0.0 {
        return 1.0;
    }
    (elapsed / t.duration).min(1.0)
}

/// The cache key an image is stored under.
pub fn image_key(img: &ImageContent) -> u64 {
    let mut h = blake3::Hasher::new();
    h.update(&[img.kind as u8]);
    h.update(img.source.as_bytes());
    u64::from_le_bytes(h.finalize().as_bytes()[..8].try_into().expect("8 bytes"))
}

/// Fits `natural` inside `dest` according to `fit`.
fn fit_rect(dest: Rect, natural: Vec2, fit: Fit) -> Rect {
    if natural.x <= 0.0 || natural.y <= 0.0 {
        return dest;
    }
    match fit {
        Fit::Fill | Fit::None => dest,
        Fit::Cover | Fit::Contain => {
            let scale = match fit {
                Fit::Cover => (dest.w / natural.x).max(dest.h / natural.y),
                _ => (dest.w / natural.x).min(dest.h / natural.y),
            };
            let w = natural.x * scale;
            let h = natural.y * scale;
            let c = dest.center();
            Rect { x: c.x - w / 2.0, y: c.y - h / 2.0, w, h }
        }
    }
}

/// True when two rects share any area.
trait Intersects {
    fn intersects(&self, other: &Rect) -> bool;
}

impl Intersects for Rect {
    fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && self.x + self.w > other.x
            && self.y < other.y + other.h
            && self.y + self.h > other.y
    }
}

/// Splits a stylesheet into `(selector, declarations)` pairs.
pub fn parse_stylesheet(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    // Strip comments first, so a comment above a rule does not eat its selector.
    let src: String = src
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");

    let mut rest = src.as_str();
    while let Some(open) = rest.find('{') {
        let selector = rest[..open].trim().to_string();
        let Some(close_rel) = rest[open..].find('}') else { break };
        let body = rest[open + 1..open + close_rel].trim();
        if !selector.is_empty() {
            out.push((selector, body.to_string()));
        }
        rest = &rest[open + close_rel + 1..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_geometry() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(Transform::default().is_identity());
        assert_eq!(Transform::default().aabb(r), r);

        // Scaling about the centre grows the box symmetrically.
        let scaled = Transform { scale: Vec2::splat(2.0), ..Default::default() };
        let a = scaled.aabb(r);
        assert!((a.w - 20.0).abs() < 0.001 && (a.h - 20.0).abs() < 0.001, "{a:?}");
        // The centre stays put: (0,0,10,10) centred at (5,5) becomes (-5,-5,20,20).
        assert!((a.x + 5.0).abs() < 0.001 && (a.y + 5.0).abs() < 0.001, "{a:?}");

        // A quarter turn keeps the extents but moves the origin.
        let rot = Transform { rotate: std::f32::consts::FRAC_PI_2, ..Default::default() };
        let a = rot.aabb(r);
        assert!((a.w - 10.0).abs() < 0.01 && (a.h - 10.0).abs() < 0.01);
    }

    #[test]
    fn stylesheet_parsing() {
        let sheet = parse_stylesheet(
            "// a comment\n.card { padding: 8px; radius: 4px; }\n#title { color: red; }",
        );
        assert_eq!(sheet.len(), 2);
        assert_eq!(sheet[0].0, ".card");
        assert!(sheet[0].1.contains("padding: 8px"));
        assert!(!sheet[0].1.contains("comment"));
        assert_eq!(sheet[1].0, "#title");
    }

    #[test]
    fn image_keys_differ_by_source() {
        let mk = |s: &str| ImageContent {
            source: s.to_string(),
            kind: ImageKind::Path,
            fit: Fit::Contain,
            tint: Color::WHITE,
            intrinsic: Vec2::ZERO,
        };
        assert_ne!(image_key(&mk("a.png")), image_key(&mk("b.png")));
        assert_eq!(image_key(&mk("a.png")), image_key(&mk("a.png")));
    }

    #[test]
    fn fit_rect_cover_and_contain() {
        let dest = Rect::new(0.0, 0.0, 100.0, 100.0);
        let nat = Vec2::new(200.0, 100.0);
        let cover = fit_rect(dest, nat, Fit::Cover);
        assert!(cover.w > 100.0 && (cover.h - 100.0).abs() < 0.01);
        let contain = fit_rect(dest, nat, Fit::Contain);
        assert!((contain.w - 100.0).abs() < 0.01 && contain.h < 100.0);
    }
}
