# Reference

Lookup tables for everything the DSL accepts. Where a table and the code
disagree, the code wins — the authoritative lists are `PROPERTIES` in
[`src/css.rs`](../src/css.rs), `TAGS` in
[`mimui-macros/src/lib.rs`](../mimui-macros/src/lib.rs), `ALL_PROPS` and
`builtins()` in [`src/anim.rs`](../src/anim.rs).

## Elements

Every element is a bare name. Anything not in the table below becomes a plain
`Box`, so a typo still renders.

| Element | Aliases | Label attribute | Notes |
| --- | --- | --- | --- |
| `MiMUI` | `root` | — | the root; defaults to `direction: column`, write `direction="row"` to change that |
| `Column` | `col`, `vstack`, `stack-column` | — | children stacked vertically |
| `Row` | `hbox`, `strip` | — | children side by side |
| `Box` | `div`, `node`, `panel`, `card` | — | plain container |
| `Label` | `text`, `p`, `span` | `text`, `label`, `title`, `value` | sized to its content |
| `Title` | `h1`, `heading` | same as `Label` | 24px, weight 700 |
| `Button` | `btn` | same as `Label` | interactive; `link`, `href`, `url`, `open` register a link |
| `Image` | `img`, `icon`, `svg`, `png`, `pic` | — | `src`, `image`, `path`, `icon`, `source`; `fit`, `tint`/`color` |
| `Progress` | `progressbar`, `bar` | — | `value` 0..1, `color`/`fill`; grows to fill its row |
| `Checkbox` | `check`, `toggle` | same as `Label` | needs `id`; optional `checked`, `color` |
| `Slider` | `range` | — | needs `id`; `value` |
| `TextInput` | `input`, `field`, `edit` | — | needs `id`; `placeholder`, `value` |
| `Spacer` | `gap`, `space`, `fill` | — | flexible empty space |
| `Divider` | `hr`, `rule`, `separator` | — | 1px rule |

The label attribute is the first of `text`, `label`, `title`, `value` that is
present, falling back to the trailing string literal. So `Label "hi"`,
`Label text="hi"` and `Label (self.name.clone())` are the same thing.

## Attributes

### Values

| Kind | Accepts |
| --- | --- |
| length | `12` (px), `50%`, `auto`, `none` |
| two lengths | `"40px 24px"` |
| colour | `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb(…)`, `rgba(…)`, and the names `transparent black white red green blue yellow orange purple magenta fuchsia cyan aqua pink gray grey silver darkgray darkgrey lightgray lightgrey navy teal olive lime brown gold indigo violet` |
| boolean | `true` / `false` / `yes` / `no` / `on` / `off` / `1` / `0`, or a bare flag (`checked`) |

Plain numbers in a colour's channel position are read as 0..255, but `0`–`1` is
taken as a float, so `rgba(0,0,0,0.5)` and `rgb(0 0 0)` both do what you expect.

### Interaction

| Attribute | Meaning |
| --- | --- |
| `id="name"` | the key for stored state. Required for `Checkbox`, `Slider`, `TextInput`. |
| `class="a b"` | space-separated class rules; later classes win |
| `style="…"` / `css="…"` | a declaration block, above inline properties |
| `on_hover="…"` | state block: `hover`, `on-hover`, `on_hover`, `:hover` |
| `on_active="…"` | `active`, `on-active`, `on_active`, `:active`, `pressed` |
| `on_focus="…"` | `focus`, `on-focus`, `on_focus`, `:focus` |
| `disabled` | flag or `disabled="…"` block; also disables children |
| `anim="…"` / `animation="…"` | keyframe shorthand; see below |
| `link="…"` | on `Button`: hands the target to `App::on_link` |

A state block names only what it changes; everything else keeps the value from
the levels below.

## Properties

Anything in this table can be written inline on a widget (`bg="#232c44"`), and
the same names work inside a `style=` block or a stylesheet.

### Box

| Property | Aliases | Values |
| --- | --- | --- |
| `display` | | `flex`, `none` |
| `position` | | `relative`, `absolute` |
| `overflow` | | `visible`, `hidden`/`clip`, `scroll`/`auto` |
| `visible` | | boolean |
| `z-index` | | integer (parsed; paint order is still tree order) |

### Layout

| Property | Aliases | Values |
| --- | --- | --- |
| `direction` | `flex-direction` | `row`/`horizontal`, `column`/`vertical` |
| `wrap` | `flex-wrap` | `wrap`, `nowrap` |
| `gap` | `row-gap`, `column-gap` | length |
| `align` | `align-items` | `start`/`flex-start`/`left`, `center`/`middle`, `end`/`flex-end`/`right`, `stretch` |
| `justify` | `justify-content` | `start`/`flex-start`/`left`, `center`/`middle`, `end`/`flex-end`/`right`, `space-between`/`between`, `space-around`, `space-evenly` |
| `align-self` | | same as `align` |
| `justify-self` | | same as `justify` |
| `flex` | | `1`, or `shrink grow [basis]` |
| `flex-grow` | | number |
| `flex-shrink` | | number |
| `flex-basis` | | length |

`flex: 1` is `1 1 0` — siblings split the space evenly rather than splitting what
is left over after their content.

### Size

| Property | Aliases | Values |
| --- | --- | --- |
| `width` / `height` | `w`, `h` | length |
| `size` | | `"w h"` |
| `min-width` `max-width` `min-height` `max-height` | | length |

A percentage inside a content-sized parent falls back to the content size
instead of collapsing to zero.

### Edges

`padding` and `margin` take one to four lengths in CSS order (top, right,
bottom, left). So does `inset`, plus `padding-top/right/bottom/left`,
`margin-top/right/bottom/left` and `top/right/bottom/left` individually.
`gap-x` sets the gap's horizontal half.

### Visuals

| Property | Aliases | Values |
| --- | --- | --- |
| `background` | `bg`, `background-color` | colour |
| `image` | `background-image` | a source path |
| `background-size` | `bg-size` | `cover`, `contain`, `fill`/`100% 100%`/`stretch`, `none`/`auto` |
| `color` | `fg`, `foreground`, `text-color` | colour — the text colour |
| `border` | `border-width` | a length, and optionally a colour: `border: 1px #2b3550` |
| `border-color` | | colour |
| `radius` | `border-radius` | 1–4 lengths, with the `/` ellipse shorthand: `radius: 10px / 4px` |
| `shadow` | `box-shadow` | `offset-x offset-y blur colour`, or `none`: `shadow: 0 6px 18px #0009` |
| `shadow-blur` `shadow-color` | | |
| `opacity` | | 0..1 |
| `clip` | | boolean — clip children to the padding box |
| `filter` / `blur` | | parsed; the renderer does not implement it yet |

### Text

| Property | Values |
| --- | --- |
| `font-size` | number (px) |
| `font-family` / `font` | family name |
| `font-weight` | `thin` `light` `normal` `medium` `semibold` `bold` `extrabold` `heavy`, `lighter`, `bolder`, or a number |
| `font-style` | `normal`, `italic` |
| `line-height` | a multiple (`1.4`) or an absolute pixel value |
| `letter-spacing` | number |
| `text-align` | `left`/`start`, `center`/`centre`, `right`/`end` |
| `word-wrap` | boolean — wraps when the node has a width |

### Transform

| Property | Aliases | Values |
| --- | --- | --- |
| `translate` | | one number or two |
| `translate-x` `translate-y` | | number |
| `scale` | | one number or two |
| `scale-x` `scale-y` | | number |
| `rotate` | `rotation` | degrees |

### Motion

| Property | Values |
| --- | --- |
| `transition` | see below |
| `transition-duration` `transition-delay` | seconds |
| `ease` / `easing` | an easing name |

## Animatable properties

What `transition:` and `anim!` can move. The name is the one to write.

| `AnimProp` | Names accepted | Value |
| --- | --- | --- |
| `X` `Y` | `x`, `y` | pixels |
| `W` `H` | `width`, `height` | pixels; grows the node's measured size |
| `Opacity` | `opacity` | 0..1 |
| `Bg` | `background`, `bg` | colour |
| `BgAlpha` | `background-alpha`, `bg-alpha` | 0..1 |
| `Fg` | `color`, `colour` | colour |
| `FgAlpha` | `color-alpha` | 0..1 |
| `Radius` | `radius`, `border-radius` | pixels |
| `BorderWidth` | `border-width` | pixels |
| `ShadowBlur` | `shadow-blur` | pixels |
| `TextX` `TextY` | `text-x`, `text-y` | pixels |
| `Scale` | `scale` | number |
| `ScaleX` `ScaleY` | `scale-x`, `scale-y` | number |
| `Rotate` | `rotate` | degrees |

`transition: all` walks this list in the order above.

## Easings

`linear`, `ease`, `ease(0.6)`, `ease-in`, `ease-out`, `ease-in-out`,
`spring(1.2)`, `bounce`, `cubic-bezier(.2,.8,.2,1)`. The default is `ease-in-out`.

The factor in `ease(f)` is the strength of the curve; the one in `spring(f)` is
the amount of overshoot.

## Built-in animations

Ready to name in `anim="…"`.

| Name | Does |
| --- | --- |
| `fade_in` `fade_out` | opacity 0↔1 |
| `pop` | scales past 1 and settles — a one-shot bounce |
| `zoom_in` `zoom_out` | scales from/to zero |
| `pulse` | opacity, twice |
| `slide_up` `slide_down` `slide_left` `slide_right` | 24px of travel plus a fade |
| `shake` | decaying horizontal shake |
| `float` | 8px up and back down, looping |
| `spin` | a full rotation |
| `wobble` | a small rotation, left and right |
| `bounce_in` | a damped drop |
| `swing` | rotates in and settles |
| `grow` | fades the background in |

## Animation shorthand

`anim="…"` is scanned token by token, so the parts may appear in any order.
Each token is classified by shape:

| Token | Meaning |
| --- | --- |
| a word that is not any of the below | the animation's name — the first such token wins |
| `0.35s`, `350ms` | the first time is the duration, the second is the delay |
| an easing name | see above |
| `infinite` | loop forever |
| a whole number | run that many times |
| `alternate`, `reverse`, `alternate-reverse` | play odd cycles backwards |

So `anim="card_in 0.5s 0.06s ease-out"` and `anim="ease-out card_in 0.5s 0.06s"`
are the same thing. A bare `0.5` with no unit is *not* a time — it falls
through to the repeat-count branch, so always write `0.5s`.

Defaults: 0.3s, no delay, `ease-in-out`, one run, forward.

In `anim!`, stop positions are fractions of the duration:

```rust
anim!(CARD_IN, [
    0.0 => { y: 24.0, opacity: 0.0 },
    1.0 => { y: 0.0,  opacity: 1.0 },
]);
```

A track that stops before `1.0` holds its last value for the rest of the run.

## Transitions

`transition="…"` is a comma-separated list. Each part is
`[property|all] [duration] [delay] [easing]`, and times **must** carry a unit
(`0.2s`, `200ms`). Across the parts the properties accumulate, the duration is
the longest one given, and the delay and easing are the last ones stated.

```
transition: "background 0.2s, scale 0.15s 0.05s ease-out"
transition: "all 0.2s"
transition: "0.2s"                 /* every property */
```

A part with no time in it contributes only its properties. With no properties
at all, every animatable property is covered.

## `App` and `WindowOptions`

```rust
fn on_link(&mut self, link: &str)          // called for `Button … link="…"`
```

`WindowOptions` builders: `title`, `size(w, h)`, `min_size(w, h)`, `theme`,
`stylesheet`, `font(bytes, family)`, `fps`, `resizable`. `fps: 0` — the
default — redraws only when something changes, including when an animation is
in flight.

## State

```rust
cx.state().get_bool(id, default)
cx.state().get_num(id, default)
cx.state().get_text(id, default)
cx.state().set_bool(id, v)   // and set_num / set_text / set
cx.state().take_links()      // drained once per frame
cx.state().flag("sidebar")   // app-level flags, plus set_flag / toggle_flag
cx.state().quit = true
cx.request_redraw()
```

Read state *after* the `ui!` block: the tree has to exist before the frame can
be applied to it.