# Contributing to MiMUI

For people working **on** MiMUI. If you are writing an app with it, the
[README](README.md) and [docs/reference.md](docs/reference.md) are what you
want.

## Start here

```sh
git clone https://github.com/mistersliva/MiMUI
cd MiMUI
cargo test                     # 60 unit + 39 integration + 17 macro + 4 doc
cargo run --example showcase   # every widget, styling and animation
```

Rust 1.85+ (edition 2024). No build script, no code generation beyond the
`ui!` proc macro, no unsafe outside `src/renderer.rs`.

Before you change anything, read [docs/architecture.md](docs/architecture.md).
It is short and it explains the two decisions that explain most of the code:
why per-frame state lives in `UiState` rather than on `Node`, and why
`Style::default()` means "unset".

## Layout of the change

A good change usually touches one of:

| If you are… | Edit | Test in |
| --- | --- | --- |
| adding a widget | `src/widget.rs`, `mimui-macros/src/lib.rs` | `tests/ui.rs` |
| adding a style property | `src/style.rs`, `src/css.rs` | `src/css.rs`, `tests/ui.rs`, `tests/doc_snippets.rs` |
| adding an animatable | `src/anim.rs` | `src/anim.rs`, `tests/ui.rs`, `tests/doc_snippets.rs` |
| changing layout | `src/layout.rs` | the `mod tests` in `src/layout.rs` |
| changing the DSL grammar | `mimui-macros/src/lib.rs` | the `mod tests` in the same file, `tests/doc_snippets.rs` |
| changing rendering | `src/renderer.rs` | *drive the showcase and look at it* |

[docs/extending.md](docs/extending.md) has the step-by-step version of each.

## Conventions

**Naming.** Widget functions are lowercase and singular (`fn button`), the tag
they answer to is capitalised (`Button`), and aliases are lowercase. CSS
property names are kebab-case and match the field on `Style` where there is an
obvious one. Keep `PROPERTIES` and `TAGS` in sync with the code — a name that
is in one and not the other is a silent no-op, not a compile error.

**Comments.** Explain *why*, and only where the code is not already obvious.
Each module opens with what it owns; the load-bearing functions carry a
paragraph explaining the rule they enforce. Match the surrounding density —
several files have one comment per public function and that is the bar.

**Doc comments.** Everything public gets one starting with the type or verb,
then a blank line, then the detail. Link with `[`Name`]` rather than writing
paths. Run `cargo doc` before you push; a broken intra-doc link is a warning
someone else has to fix.

**Error handling.** Parsers return `Option` or a small `Result`. Rendering
failures surface as `Result<_, String>` because they happen once at startup.
Nothing in the frame path allocates an error.

**Tests.** `#[test]` functions with a lowercase sentence that says what is
being pinned down — `a_state_block_does_not_leak_into_the_base_style` — not
`test_cascade_2`. Arrange/act/assert with a blank line between the phases and a
message on the assertion that says what went wrong.

## Testing

Three layers, and the choice matters.

**Unit tests, in the same file as the code.** For anything with interesting
logic in isolation: layout arithmetic, easing curves, the CSS parser, the DSL
parser. They are the fastest to write and the easiest to trust.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_stacks_top_to_bottom() {
        let s = st("direction: column; gap: 4px;");
        // …
    }
}
```

**Integration tests, in `tests/ui.rs`.** For anything that spans the pipeline —
cascade, layout with real widgets, animation across frames, interaction. The
`Harness` mirrors `draw_frame`'s order and calls `input.end_frame()`; copy it
rather than inventing a second one.

```rust
let mut h = Harness::new();
h.frame_with(
    viewport(), dark(),
    |_| {},                                    // setup: classes, pointer
    |cx| { ui! { MiMUI { … } } },              // the app body
    |cx| { /* read rectangles: layout has run */ },
);
```

Two rules, both learned the hard way:

- **The `inspect` closure runs after layout and before interaction.** It is the
  only place rectangles are final. Reading a rect inside the body closure gets
  you zeros.
- **`inspect` cannot touch the harness**, which is already borrowed. Capture
  what you need into locals declared before the call.

**Driving the real window.** Some defects are invisible to the harness:
colour space, the swapchain, the event loop, the glyph atlas, anything about
presentation. For those, run the showcase, interact with it, and *look*. Then
drive the window from PowerShell and read the pixels back:

```powershell
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap 1920, 1080
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
$bmp.Save("shot.png", [System.Drawing.Imaging.ImageFormat]::Png)
```

Sampling a pixel and comparing it to the colour you wrote is the fastest way to
catch a gamma or format mistake:

```powershell
$bmp.GetPixel(536, 503)   # expect exactly #4a7fff
```

**Docs are tested too.** `tests/doc_snippets.rs` builds every element, property
value, easing and `anim=` spelling that `docs/reference.md` claims to accept.
If you change the grammar or the property table, that test is what notices — so
keep the docs and the tables in the same commit, and add the new spelling to the
snippet file in the same breath.

**A clean build is part of the test.**

```sh
cargo build 2>&1 | Select-String warning    # no warnings
```

## Known sharp edges

Things that will bite you if you do not know about them. The full list is at
the end of [docs/extending.md](docs/extending.md).

- **`Style::default()` is the identity.** A field whose default is a *sensible
  value* rather than *unset* will never be filled in by `merge_defaults`, and
  will block `merge_partial` from working.
- **The tree is rebuilt every frame.** Anything that crosses a frame belongs in
  `UiState`, keyed by element `id`.
- **A widget does not close its own node.** `UiCtx::end` does. Use
  `cx.close_node(i)` when you need a sibling, or the next element gets
  re-parented.
- **`measure_text` has to be on** for a node's text to be shaped, and unshaped
  text draws nothing.
- **A state block must not be read as an inline property.** `on_hover` and
  friends are excluded from that path in `Resolved::from_spec`; if you add
  another block-shaped attribute, exclude it too, or its declarations leak into
  the always-on style.
- **Coordinates are logical everywhere except the renderer.** `DrawCmd` rects,
  clip bounds and glyph quads are logical; vertex positions are physical. The
  fragment shader divides by `Globals::scale` before testing the clip.
- **Colours are sRGB; the surface is sRGB.** Both fragment stages and the clear
  convert to linear first. Anything new that writes colour has to as well.

## Commit messages

One commit does one thing. The subject is a sentence in the imperative —
`Add a Badge widget`, not `Added badge` — and the body explains why the change
was needed, not what the diff already says. If a bug was found by running the
app, say so, and list the other defects it turned up; that is the part a reader
cannot get from the diff.

## Opening a pull request

1. `cargo test` and `cargo build` clean.
2. New behaviour has a test that fails without it.
3. Public API changes have doc comments.
4. A change to rendering or the frame loop has a screenshot before and after.
5. The README and `docs/` still describe what the code does.

## Layout of this document

- [docs/architecture.md](docs/architecture.md) — how a frame works, module map,
  the `UiState` rule.
- [docs/extending.md](docs/extending.md) — recipes for adding a widget, a
  property, an animatable; theming; fonts; custom drawing; what is not wired up.
- [docs/reference.md](docs/reference.md) — every element, attribute, property,
  easing and built-in animation in one place.