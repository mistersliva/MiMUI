# MiMUI

An immediate-mode GUI library for Rust, written from scratch on `wgpu`.

The whole point is that a UI reads like markup, styles like CSS, and moves
without a state machine:

```rust
use mimui::prelude::*;

struct MyApp;

impl App for MyApp {
    fn ui(&mut self, cx: &mut UiCtx<'_>) {
        ui! { MiMUI {
            Label "MiMUI";
            Button "Click!" link="openoptionswindow";
            Button class="bottom-btn" "github" link="github.com";
        } }
    }
}

fn main() {
    mimui::run(MyApp, WindowOptions::default().title("hello").size(640, 480));
}
```

No `ui.button(...)` prefixes, no builder chains, no retained widget tree: you
describe the interface you want *this frame* and MiMUI works out the rest.

```sh
cargo run --example showcase
```

## The syntax

`ui! { … }` is a macro, so the DSL is parsed from the token stream. A widget is
a bare name; its label is a string right after it; everything else is
`name="value"`:

```rust
ui! { MiMUI {
    Column gap="16px" padding="24px" {
        Title "Settings"

        Row gap="10px" align="center" {
            Checkbox id="notify" label="Notify me" checked
            Slider id="volume" value=0.6
        }

        TextInput id="name" placeholder="your name…"

        Row gap="8px" {
            Button "Cancel"
            Button class="primary" "Save" link="save"
        }
    }
} }
```

- Children go in `{ }`, attributes go before or after — both parse.
- `;` after an element is optional when the next token starts a new widget.
- A trailing string is the label, so `Button "Save"` and
  `Button text="Save"` are the same thing.
- `(expr)` interpolates a Rust value: `Label (self.user.name.clone())`.
- Bare flags work too: `checked`, `disabled`, `focusable`.

Widgets: `Label` `Title` `Button` `Checkbox` `Slider` `TextInput` `Image`
`Progress` `Divider` `Spacer`, and the layout containers `Column` `Row` `Box`.
Anything unrecognised becomes a plain `Box`, so a typo still draws something.

## Styling

Style attributes are CSS properties written inline:

```rust
Box w="100px" h="40px" radius="10px" bg="#232c44"
```

Or keep the tree readable and use classes:

```rust
ui! { MiMUI {
    Column {
        Box class="card" { Label "hi" }
    }
} }
```

```
.card {
    background: #1b2233;
    border-width: 1px;
    border-color: #2b3550;
    radius: 14px;
    padding: 14px 16px;
}
```

Cascade order, lowest to highest:

1. the widget's own look (a `Button` is blue and rounded by default)
2. each `class`, left to right
3. inline properties written on the widget
4. a `style="…"` block
5. an `on_hover` / `on_active` / `on_focus` / `disabled` block, when that state
   is current

Anything a state block does not mention keeps its value from the lower
levels, so `on_hover="background: #4a7fff"` changes one thing.

The full property list is in `src/css.rs` (`css::known_properties`). It covers
the usual flexbox set — `direction`, `gap`, `align`, `justify`, `flex`,
`width`/`height` (px, %, auto), `padding`/`margin`, `inset`, `overflow`,
`radius`, `border`, `shadow`, `opacity`, `color`, `font-*`, `line-height`,
`letter-spacing`, `text-align`, `word-wrap`, `translate`, `scale`, `rotate`,
`cursor`, `transition`.

## Animation

Two kinds, both declarative.

**Transitions** are automatic. Write one and any change to the properties it
covers is interpolated:

```rust
Box bg="#232c44"
    transition="background 0.2s, scale 0.15s"
    on_hover="background: #4a7fff; scale: 1.04;"
```

**Keyframes** are named recipes you register once, at startup:

```rust
fn register_animations() {
    anim!(CARD_IN, [
        0.0 => { y: 24.0, opacity: 0.0 },
        1.0 => { y: 0.0,  opacity: 1.0 },
    ]);
}
```

and then use them by name:

```rust
Column class="card" anim="card_in 0.5s ease-out"
Button "pulse" anim="pulse 1.2s infinite"
```

The shorthand is `name duration [delay] [easing] [count] [alternate]`, e.g.
`"pop 0.3s 0.1s ease-out 3 alternate"`. Positions in `anim!` are fractions of
the duration, so `0.0` is the start, `1.0` the end and `0.5` halfway; a track
may stop early to hold its last value.

Built-in recipes: `fade_in`, `fade_out`, `pop`, `zoom_in`, `slide_up`,
`bounce_in`, `pulse`, `shake`, `spin`.

Animatable properties: `x` `y` `w` `h` `opacity` `background` `color`
`radius` `border-width` `shadow-blur` `scale` `rotate`.

MiMUI redraws only when something changes — a running animation, a state
switch, an input event — so an idle window costs nothing.

## Images

```rust
Image src="assets/logo.png" w="64px" h="64px" fit="contain"
Image src="assets/logo.png" tint="#4a7fff" radius="12px"
```

`fit` is one of `contain`, `cover`, `fill`, `none`, `scale-down`. Decoding
happens off the UI thread's critical path: the first frame reports the image
as missing, the renderer decodes and uploads it, and the next frame draws it.
PNG, JPEG, WebP and GIF are built in; SVG needs the default-on `svg` feature.

## State

Widgets keep their value between frames, keyed by `id`:

```rust
impl App for MyApp {
    fn ui(&mut self, cx: &mut UiCtx<'_>) {
        ui! { MiMUI {
            Checkbox id="notify" label="Notify me"
            Slider id="volume"
            TextInput id="name"
        } }

        // Read it back after building the tree.
        self.notify = cx.state().get_bool("notify", false);
        self.volume = cx.state().get_num("volume", 0.5);
    }
}
```

`Button "…" link="…"` is the same mechanism: `App::on_link` receives the
target on the next frame, and anything that looks like a URL opens in the
browser.

```rust
fn on_link(&mut self, link: &str) {
    match link {
        "save" => self.save(),
        "openoptionswindow" => self.show_options(),
        url => mimui::open_in_browser(url),
    }
}
```

## How it fits together

| File | What it does |
| --- | --- |
| `mimui-macros/` | Parses the `ui!` token stream into element/attribute lists |
| `src/ctx.rs` | Builds the node tree, resolves styles, lays out, emits draw commands |
| `src/style.rs`, `src/css.rs` | The style struct, the property table, the cascade |
| `src/layout.rs` | Flexbox: two passes, grow/shrink, wrap, absolute children |
| `src/anim.rs`, `src/easing.rs` | Keyframes, transitions, easing curves |
| `src/text.rs` | Font discovery, shaping, the glyph atlas |
| `src/image.rs` | PNG/SVG decoding |
| `src/renderer.rs` | wgpu: one SDF shader for boxes, one textured shader for images |
| `src/app.rs` | The window, the event loop, the frame |

The tree is rebuilt every frame, so anything the *next* frame needs —
positions, resolved styles, running animations, in-flight transitions — is
looked up by element `id` in `UiState` rather than carried on a node. That is
why `UiCtx` is cheap to throw away.

## Building

```sh
cargo build --release
cargo test
cargo run --example showcase
```

Needs Rust 1.85+ (edition 2024). Rendering is `wgpu` 30, so a Vulkan, Metal or
DX12 backend is required at runtime.

## Examples

- `examples/showcase.rs` — every widget, both styling techniques, all the
  animation styles.
- `examples/weights.rs` — the same string at four font weights.

## Licence

MIT.