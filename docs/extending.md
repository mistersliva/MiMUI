# Extending MiMUI

Recipes for adding to the library. Each one lists every file that has to change
— the usual failure mode is changing three of the four and finding out at
runtime.

Read [architecture.md](architecture.md) first if you have not.

## Add a CSS property

Say you want `border-style: dashed`. Five steps.

**1. Add the field to `Style`** in [`src/style.rs`](../src/style.rs). Its
`Default` **must** be the identity — a value meaning "nobody said" — because
`merge_partial` and `merge_defaults` both work by comparing against
`Style::default()`.

```rust
pub enum BorderStyle { None, Solid, Dashed }

pub struct Style {
    // ...
    pub border_style: BorderStyle,
}
```

For an enum, derive `Default` and mark the unset variant:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum BorderStyle {
    #[default]
    None,
    Solid,
    Dashed,
}
```

**2. Add the parse arm** in `apply_property` ([`src/css.rs`](../src/css.rs)):

```rust
"border-style" => s.border_style = border_style_of(value).ok_or_else(bad)?,
```

**3. List every alias** in `PROPERTIES` ([`src/css.rs`](../src/css.rs)). This
array is what lets the property be written inline on a widget —
`Box border-style="dashed"` is parsed as a property because
`css::is_property` consults it. Adding the parse arm without the alias means the
property only works inside a `style=` block.

**4. If it should animate**, add an `AnimProp` — see the next section.

**5. If the renderer has to know about it**, extend the draw command and the
WGSL. `border-style: dashed`, for instance, means `DrawCmd::Box` needs the
value and `fs_box` needs a dash pattern.

Add a unit test in `src/css.rs`'s test module that parses the property, and one
in `tests/ui.rs` that writes it inline on a widget.

## Add an animatable property

`src/anim.rs`. Five places, all keyed off the same `AnimProp` variant:

```rust
pub enum AnimProp { /* … */ MyProp }

const ALL_PROPS: &[AnimProp] = &[/* … */, AnimProp::MyProp];

impl AnimProp {
    fn name(self) -> &'static str { /* MyProp => "my-prop" */ }

    fn from_name(s: &str) -> Option<Self> {
        // "my-prop" | "myprop" => Some(AnimProp::MyProp)
    }

    fn read(self, s: &Style) -> AnimVal { /* pull the value out */ }

    fn write(self, s: &mut Style, v: AnimVal) { /* push it in */ }
}
```

`read`/`write` must be exact inverses, and `read` must return the *same*
`AnimVal` for equal values — `start_transitions` compares them with `==` to
decide whether a tween is needed. A float property read as `AnimVal::Num`
rounds consistently, so that is fine; a colour read as `AnimVal::Color` is
compared channel-wise, which is also fine.

Order matters in `ALL_PROPS`: it is the order `transition: all` walks, and it
determines which tween wins when two of them touch the same value.

Once it is there, `transition: "my-prop 0.2s"` and `anim!("x", [0.0 => { my-prop: 1.0 }])`
both work.

## Add a widget

**Where this happens.** Widgets are functions in
[`src/widget.rs`](../src/widget.rs). There is no runtime registration: the two
helpers below, `open` and `finish`, are file-local on purpose, so adding a
widget means editing the library, not implementing a trait. The types it takes
— `WidgetSpec` and `Resolved` — are public, but only so that
`mimui::widget::dispatch` has a signature you can read.

**1. Write it**, next to the others. Inside that file `Corners`, `Dim`, `Edges`
and `Color` are already in scope.

```rust
/// A `Badge`: a small pill that shows a count.
fn badge(cx: &mut UiCtx<'_>, r: &mut Resolved, spec: &WidgetSpec<'_>) {
    let mut d = Style::default();
    d.padding = Edges {
        top: Dim::Px(2.0),
        right: Dim::Px(6.0),
        bottom: Dim::Px(2.0),
        left: Dim::Px(6.0),
    };
    d.radius = Corners::splat(999.0);
    d.background = Some(Color::rgb(74, 127, 255, 1.0));
    d.color = Color::WHITE;
    d.font_size = 11.0;
    d.flex_shrink = 0.0;
    r.merge_defaults(&d);

    let text = spec.label();
    let n = open(cx, r, "badge");
    cx.set_text(n, text, false);
    cx.set_measure_text(n, true);      // without this it draws no glyphs
    finish(cx, r, n);
}
```

The signature is `fn name(cx: &mut UiCtx<'_>, r: &Resolved, spec: &WidgetSpec<'_>)`
— take `&mut Resolved` when you need to merge defaults in.

Rules the existing widgets follow, and why:

- **`merge_defaults(&d)` first.** It fills in anything the user left at the
  identity value *and* propagates the new value into the `on_hover` slots, so a
  state block still inherits the widget's padding. Merging defaults after the
  state blocks are built silently breaks hover styling.
- **The `id` must live on the node that holds the value.** For a widget whose
  value is a child node — the checkbox is the obvious one — copy the id onto the
  child and give the wrapper a generated one, so `id="agree"` cannot end up
  ambiguous.
- **Call `cx.close_node(i)` after any sibling subtree.** Otherwise the next
  element is parented to your last child instead of to you.
- **`finish` does not close the node.** `UiCtx::end` does that when the
  element's block closes, which is what lets `{ … }` children attach to you.

**2. Register it** in `dispatch` in the same file:

```rust
"badge" | "pill" => badge(cx, &mut r, spec),
```

The unknown-tag arm already falls through to a plain `Box`, so a widget works
in the DSL the moment `dispatch` knows it — with default styling only.

**3. Add the tag to `TAGS`** in
[`mimui-macros/src/lib.rs`](../mimui-macros/src/lib.rs). This list is how the
parser knows `Badge "3" Button "ok"` is two elements rather than one element
with a stray flag. Forgetting it is not a compile error: you get one `Badge`
with `Button` as a boolean attribute and the second widget silently missing.

**4. Test it.** At minimum: it renders something, its label appears, and its
default styling is overridden by an inline property.

```rust
#[test]
fn a_badge_sizes_to_its_count() {
    let mut h = Harness::new();
    let mut rect = Rect::ZERO;
    h.frame_with(viewport(), dark(), |_| {}, |cx| {
        ui! { MiMUI { Row { Badge "12" } } }
    }, |cx| {
        let b = cx.find_by_tag("badge").expect("the badge");
        rect = cx.nodes()[b].rect;
    });
    assert!(rect.w > 10.0, "{rect:?}");
}
```

Use the `Harness` in [`tests/ui.rs`](../tests/ui.rs) — it mirrors the real
pipeline, including `input.end_frame()`, and a test that skips it will hang on
a press forever.

## Theming

Two levels.

**A theme** is a `Style` merged underneath the `MiMUI` root, so it sets the
colour and font defaults for everything:

```rust
mimui::run(app, WindowOptions::default().theme(
    Style::parse("background: #10131c; color: #eef2ff; font-size: 14px;"),
))
```

**Classes** are rules by name, registered with `cx.add_class` or through
`WindowOptions::stylesheet`. They apply per element, in the order written on the
element:

```rust
.button-primary { background: #4a7fff; color: white; }
<Box class="button button-primary" />   /* `button-primary` wins */
```

Both live below inline properties, a `style=` block and any state block. See
`Resolved::from_spec` in [`src/widget.rs`](../src/widget.rs) for the exact
order — it is five numbered steps and worth reading before changing it.

## Fonts

MiMUI drops non-text faces from the font database at startup
(`drop_non_text_fonts` in [`src/text.rs`](../src/text.rs)). This is not
cosmetic: cosmic-text 0.12 ranks candidate faces by weight alone, so on Windows
an icon font registered at weight 500 wins and `font-weight: 500` renders
`Hamburgefonstiv` as pictograms. `examples/weights.rs` reproduces it.

To ship your own font, hand the bytes over and name the family:

```rust
mimui::run(app, WindowOptions::default()
    .font(include_bytes!("Inter.ttf").to_vec(), "Inter"));
```

That makes it the default family. `TextEngine::retain_families` narrows what
the engine will consider, if the default filter is not what you want.

## Draw your own thing

If a widget needs pixels the library does not draw — a custom chart, a
gradient, a mesh — the cheapest path is a `DrawCmd`. Add a variant in
[`src/ctx.rs`](../src/ctx.rs):

```rust
pub enum DrawCmd {
    // …
    /// A run of line segments in logical pixels.
    Lines { points: Vec<Vec2>, width: f32, color: Color, clip: Option<Rect> },
}
```

then handle it in `Renderer::build_vertices`. You need to:

- push geometry into the single shared vertex buffer;
- extend the batch key, or reuse the box pipeline if your geometry can be
  expressed as quads;
- respect `clip` — it is in logical pixels, vertex positions in physical ones.

`Frame` is the only thing the renderer sees. As long as a command round-trips
from `emit` through `build_vertices` into a draw, the rest of the pipeline
interactions and clipping does not care.

## Things that are parsed but not wired up

Be aware of these before promising them:

| Thing | State |
| --- | --- |
| widget registration | there is none; widgets are functions in `src/widget.rs` |
| `overflow: scroll` | parses; nothing reads `input.scroll_delta` into `Node::scroll` |
| `filter` / `blur` | carried in draw commands, ignored by the renderer |
| `align: baseline` | accepted as a keyword, behaves as `start` |
| `fit: scale-down` | not in the `Fit` parser (`cover`, `contain`, `fill`, `none` only) |
| `z-index` | parsed and ignored; paint order is tree order |

Each is a small job if you want it. Wiring up scrolling means reacting to
`input.scroll_delta` against the hovered scrollable node and writing
`Node::scroll`, which `flex` already honours and `apply_clip` already accounts
for — the layout side is already there.