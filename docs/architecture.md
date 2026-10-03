# Architecture

How a MiMUI frame is put together, and why it is shaped the way it is. Read
this before changing anything; [extending.md](extending.md) assumes it.

## The one constraint that shapes everything

MiMUI is immediate mode: there is no retained widget tree. Every frame the app
calls `ui! { … }` and gets a *brand-new* `Vec<Node>` — the nodes from last frame
are gone, along with their positions, their resolved styles, their running
animations and their in-flight transitions.

So anything frame N needs at frame N+1 cannot live on a node. It lives in
[`UiState`](src/state.rs), keyed by the element's `id`:

| Field | Holds | Why |
| --- | --- | --- |
| `values` | widget values (bool / num / text / link) | the checkbox's on-off state |
| `rects` | last frame's rectangle, per `id` | hit-testing and parent sizing need a position *before* layout runs |
| `prev_styles` | last frame's resolved style | transitions interpolate from here |
| `tween_state` | in-flight transitions | a tween that restarted every frame would never advance |
| `running` | keyframe animations, keyed by `(id, name)` | the same: an animation re-declared each frame must keep its clock |
| `focus` | the focused element's `id` | indices do not survive; ids do |

Every bug in that list was found by running the showcase, not by reading the
code, and each has a regression test in `tests/ui.rs`. If you add state that
crosses a frame, it goes in `UiState`, not on `Node`.

`UiCtx` itself is deliberately cheap to throw away: it borrows
`InputState` / `UiState` / `TextEngine` / `Clock` and owns nothing that
outlives the frame.

## The pipeline

`Shell::draw_frame` in [`src/app.rs`](src/app.rs) runs these in order. A test
harness that wants to mirror it should copy the order exactly — several bugs
only appear when the order is right.

```
 1. advance the clock (dt is clamped to 100ms)
 2. app.update(dt)                      app-side logic, no UI
 3. UiCtx::new(input, state, text, clock, viewport, scale)
 4. cx.set_theme(theme); cx.add_stylesheet(&stylesheet)
 5. app.ui(&mut cx)                     ← the ui! macro builds the tree here
 6. cx.resolve_styles(dt)               pick a style per node, start transitions
 7. cx.measure_text()                   shape text, record intrinsic sizes
 8. cx.layout(viewport)                 size bottom-up, position top-down
 9. cx.handle_interactions()            hit-test, activate, update widget values
10. cx.build_frame()                    walk the tree, emit DrawCmd list
11. links → app.on_link
12. upload the glyph atlas / any new images
13. renderer.render(&frame, viewport, scale)
14. state.prune(); input.end_frame()
```

### 5. Building the tree

`ui!` lowers to `UiCtx::elem(tag, attrs, is_root)` … `UiCtx::end()`, one pair per
element. Two independent things are tracked:

- the **macro stack** (`UiCtx::stack`) remembers the parent so the matching
  `end()` can restore it;
- the **node tree** (`Node.parent` / `Node.children`) is what layout, hit-testing
  and painting walk.

`elem` pushes onto the macro stack *before* dispatching. That is why a widget
must not close its own node: children written inside `{ … }` have to land on the
widget's node. A widget that needs siblings must say so:

```rust
let track = cx.push_node(&id, "slider-track", styles);
cx.close_node(track);          // back to the slider, ready for the fill
```

`close_node(i)` makes node `i`'s parent current, whether or not `i` was current.
That matters when `i` has children of its own — the checkbox's tick, for
instance. Forgetting one of these calls silently re-parents the next element,
which shows up as a collapsed layout rather than an error.

### 6. Resolving styles

For each node, in index order:

1. compute the interaction `TriggerSet` (hover / active / focus / disabled)
   from the rectangle **seeded from last frame** — the current frame has not
   been laid out yet;
2. `styles.pick(&set)` picks the base or the most specific state block;
3. if there is a previous style, `start_transitions` compares the two
   property-by-property and starts a tween per difference the node's
   `transition:` covers;
4. every animation in `UiState::running` for this `id` advances and writes its
   sampled values;
5. in-flight tweens are applied last — they win over animations;
6. the result is stored in three places: `Node.resolved`, `Node.previous`, and
   `UiState::prev_styles`.

### 7. Measuring text

`measure_text` shapes every node with `measure_text` set, using cosmic-text,
and rasterises any glyph not already in the atlas. **A node with text but no
shaping draws nothing**, which is easy to get wrong: the flag has to be on even
when the size is fixed.

Glyph ink boxes are stored relative to the **pen and the baseline**, never to
the line box. The shaper supplies `x` (pen) and `y` (baseline within the line),
and the rasteriser supplies the bearing and the ascent offset. Mixing those two
kinds of offset is what made glyphs drift.

### 8. Layout

Two passes over a post-order walk (`src/layout.rs`):

- **sizes, bottom-up.** Each node's content size is
  `measure(style, own_content, children_sizes, parent_available)`, plus its
  margins. `own_content` comes from shaped text or an image's intrinsic size.
- **positions, top-down.** Each container hands its border box to `flex`,
  which wraps, grows, shrinks, justifies and aligns its children in one go.

Three things to know about `flex`:

- it insets by `padding` itself, so hand it the **border box**, not the content
  box (`content_box` is only for measuring);
- a percentage resolves against the parent's available size, and when that size
  is zero — a content-sized parent — it falls back to the content size instead
  of collapsing to nothing;
- `flex: 1` means `1 1 0`, so siblings share the space evenly rather than
  splitting whatever is left over.

Layout finishes by pushing clip bounds down the tree (`apply_clip`) and writing
every rectangle into `UiState::rects`.

### 9. Interaction

`handle_interactions` finds the topmost interactive node under the pointer —
later nodes paint on top, so the last hit wins — and then:

- **press** captures the pointer and focuses the node. It does *not* activate;
- **release** activates, and only if the pointer is still on the same node, so
  dragging off cancels a click;
- a slider is the exception: it activates on press and keeps following the
  pointer for the whole drag;
- `Tab` walks the interactive elements in tree order, wrapping around;
- `Enter` / `Space` activates the focused element; typing goes to a focused
  text input.

### 10. Building the frame

`build_frame` walks the tree in paint order and emits three kinds of command:
`Box` (fill / border / shadow), `Image` (glyphs and pictures), and the clear
colour plus the list of images that still need decoding. `Frame::needs_redraw`
is true when an animation or transition is in flight, which is what keeps the
loop awake.

## Rendering

Two pipelines over one vertex buffer.

- **`fs_box`** — a rounded-rectangle SDF, so radii, borders and soft shadows
  come out of one quad each. The vertex carries the shape-space position, the
  half-extent and the radius; the fragment shader solves the distance function
  and antialiases across about one pixel.
- **`fs_image`** — samples a texture and multiplies by the tint's alpha.

Draw commands are expanded to triangles and batched by *(pipeline, texture)*.
Because boxes and images interleave in paint order, batching only merges
**contiguous** runs; reordering them would break overlap.

Colours are authored in sRGB, the surface is an `*-srgb` format, and the
hardware re-encodes whatever it is handed. Both fragment stages convert to
linear first, and so does the clear colour, which bypasses the shader. Skipping
this lightens every colour — `#ff6b6b` comes out as `#ffadad`.

Clip bounds are in logical pixels, vertex positions in physical ones; the
fragment shader divides by `Globals::scale` before testing.

## Reading the code

| Module | Owns |
| --- | --- |
| `mimui-macros` | the `ui!` token-stream parser and code generator |
| `src/ctx.rs` | `UiCtx`, `Node`, `Frame`, `DrawCmd`, the pipeline above |
| `src/style.rs` | `Style`, `TriggerStyles`, `TriggerSet`, `State`, `merge_partial` |
| `src/css.rs` | the property table: parsing and the property list |
| `src/layout.rs` | `measure` and `flex` |
| `src/anim.rs` | `AnimProp`, `Keyframes`, `Animation`, `Transition`, `anim!` |
| `src/easing.rs` | the easing curves and their parser |
| `src/text.rs` | font discovery, shaping, the glyph atlas |
| `src/image.rs` | PNG/JPEG/WebP/GIF and SVG decoding |
| `src/widget.rs` | the widget library: dispatch and one function per widget |
| `src/renderer.rs` | wgpu: pipelines, the WGSL, batching, uploads |
| `src/app.rs` | `App`, `WindowOptions`, `Shell`, the frame loop |
| `src/geom.rs`, `src/color.rs`, `src/input.rs`, `src/state.rs` | primitives |

`Style::default()` is the **identity**: a value equal to the default means
"nobody said". That is what lets `merge_partial` overlay a declaration block
onto an inherited style, and it is why widget defaults are merged by comparing
against it. If you add a field, its `Default` has to mean "unset", not
"sensible value" — otherwise `merge_defaults` will never fill it in.