//! End-to-end tests: build a UI tree without a window and inspect the frame.
//!
//! These exercise the whole pipeline except the GPU draw call, so a regression
//! in the cascade, layout, animation or draw-list emission shows up here.

use mimui::input::InputState;
use mimui::prelude::*;
use mimui::text::TextEngine;
use mimui::{AnimProp, AnimVal};

/// Everything a frame needs, plus the mutable inputs so tests can inspect them.
struct Harness {
    input: InputState,
    state: UiState,
    text: TextEngine,
    clock: Clock,
}

impl Harness {
    fn new() -> Self {
        Self {
            input: InputState::new(),
            state: UiState::new(),
            text: TextEngine::new(),
            clock: Clock::default(),
        }
    }

    /// Runs one UI pass at `dt` seconds per frame.
    ///
    /// * `setup` runs before the app body — class registration, pointer moves.
    /// * `inspect` runs after layout, which is the first point at which
    ///   rectangles are final. It must not touch the harness, which is already
    ///   borrowed; capture what you need into locals instead.
    #[allow(clippy::too_many_arguments)]
    fn run(
        &mut self,
        dt: f32,
        viewport: Vec2,
        theme: Style,
        setup: impl FnOnce(&mut UiCtx<'_>),
        body: impl FnOnce(&mut UiCtx<'_>),
        inspect: impl FnOnce(&UiCtx<'_>),
    ) -> Frame {
        self.clock.dt = dt;
        self.clock.now += dt;

        let frame = {
            let Harness { input, state, text, clock } = self;
            let mut cx = UiCtx::new(input, state, text, clock, viewport, 1.0);
            cx.set_theme(theme);
            setup(&mut cx);
            body(&mut cx);
            cx.resolve_styles(dt);
            cx.measure_text();
            cx.layout(Rect::new(0.0, 0.0, viewport.x, viewport.y));
            inspect(&cx);
            cx.handle_interactions();
            cx.build_frame()
        };

        // Without this the press/release edge sets never clear, and a single
        // click would look like a press that is held forever.
        self.input.end_frame();
        frame
    }

    /// One frame at 60 Hz.
    fn frame(
        &mut self,
        viewport: Vec2,
        theme: Style,
        body: impl FnOnce(&mut UiCtx<'_>),
    ) -> Frame {
        self.run(1.0 / 60.0, viewport, theme, |_| {}, body, |_| {})
    }

    /// One frame with setup and inspection hooks.
    #[allow(clippy::too_many_arguments)]
    fn frame_with(
        &mut self,
        viewport: Vec2,
        theme: Style,
        setup: impl FnOnce(&mut UiCtx<'_>),
        body: impl FnOnce(&mut UiCtx<'_>),
        inspect: impl FnOnce(&UiCtx<'_>),
    ) -> Frame {
        self.run(1.0 / 60.0, viewport, theme, setup, body, inspect)
    }
}

fn dark() -> Style {
    Style::parse("background: #10131c; color: white;")
}

fn viewport() -> Vec2 {
    Vec2::new(400.0, 300.0)
}

/// Boxes drawn with a strongly red fill.
fn red_boxes(frame: &Frame) -> usize {
    frame
        .cmds
        .iter()
        .filter(|c| matches!(c, DrawCmd::Box { fill: Some(f), .. } if f.r > 0.9 && f.b < 0.1))
        .count()
}

/// The rectangles of every node whose `tag` matches, in tree order.
fn rects_of(cx: &UiCtx<'_>, tag: &str) -> Vec<Rect> {
    (0..cx.nodes().len())
        .filter(|&i| cx.nodes()[i].tag == tag)
        .map(|i| cx.nodes()[i].rect)
        .collect()
}

/// The text a node is showing, if any.
fn text_of(cx: &UiCtx<'_>, tag: &str) -> Option<(String, bool)> {
    let i = cx.find_by_tag(tag)?;
    let t = cx.nodes()[i].text.as_ref()?;
    Some((t.text.clone(), t.placeholder))
}

// ---------------------------------------------------------------------------
// rendering a tree
// ---------------------------------------------------------------------------

#[test]
fn a_label_draws_a_box_and_glyphs() {
    let mut h = Harness::new();
    let frame = h.frame(viewport(), dark(), |cx| {
        ui! { MiMUI {
            Column padding="20px" {
                Label "hello"
            }
        } }
    });

    let boxes = frame.cmds.iter().filter(|c| matches!(c, DrawCmd::Box { .. })).count();
    let glyphs = frame
        .cmds
        .iter()
        .filter(|c| matches!(c, DrawCmd::Image { texture: TextureRef::GlyphAtlas, .. }))
        .count();
    assert!(boxes >= 1, "the label background should draw a box");
    assert!(glyphs >= 4, "five letters should draw glyph quads, got {glyphs}");
    assert_eq!(frame.clear.to_u32(), 0x1013_1cff);
}

#[test]
fn a_button_is_sized_to_its_label_and_picks_up_its_style() {
    let mut h = Harness::new();
    let mut rect = Rect::ZERO;
    let mut style = Style::default();
    h.frame_with(
        viewport(),
        dark(),
        |_| {},
        |cx| {
            ui! { MiMUI {
                Column padding="10px" align="start" {
                    Button id="go" "Go"
                }
            } }
        },
        |cx| {
            let n = cx.find_by_tag("button").expect("the button node exists");
            rect = cx.nodes()[n].rect;
            style = cx.nodes()[n].resolved.clone();
        },
    );

    assert!(rect.w > 20.0 && rect.h > 20.0, "button sized to its text: {rect:?}");
    assert!(rect.x >= 10.0, "padding insets the button: {rect:?}");
    assert_eq!(style.background, Some(Color::rgb(64, 82, 122, 1.0)));
    assert_eq!(style.cursor, mimui::style::CursorKind::Pointer);
}

// ---------------------------------------------------------------------------
// layout
// ---------------------------------------------------------------------------

#[test]
fn children_stack_in_a_column() {
    let mut h = Harness::new();
    let mut rects = Vec::new();
    h.frame_with(
        viewport(),
        dark(),
        |_| {},
        |cx| {
            ui! { MiMUI {
                Column gap="20px" align="start" {
                    Label "a"
                    Label "b"
                    Label "c"
                }
            } }
        },
        |cx| rects = rects_of(cx, "label"),
    );

    assert_eq!(rects.len(), 3);
    assert!(rects[1].y >= rects[0].y + rects[0].h + 19.0, "gap applies: {rects:?}");
    for r in &rects {
        assert!((r.x - rects[0].x).abs() < 0.01, "a column shares one x: {rects:?}");
    }
}

#[test]
fn children_sit_side_by_side_in_a_row() {
    let mut h = Harness::new();
    let mut rects = Vec::new();
    h.frame_with(
        viewport(),
        dark(),
        |_| {},
        |cx| {
            ui! { MiMUI {
                Row gap="10px" {
                    Label "aa"
                    Label "bb"
                }
            } }
        },
        |cx| rects = rects_of(cx, "label"),
    );

    assert_eq!(rects.len(), 2);
    assert!(rects[1].x >= rects[0].x + rects[0].w + 9.0, "gap applies: {rects:?}");
    assert!((rects[0].y - rects[1].y).abs() < 0.01, "a row shares one y");
}

#[test]
fn nested_flex_containers_stay_inside_their_parent() {
    let mut h = Harness::new();
    let mut boxes = Vec::new();
    h.frame_with(
        viewport(),
        dark(),
        |_| {},
        |cx| {
            ui! { MiMUI {
                Column padding="20px" gap="10px" {
                    Row gap="8px" {
                        Box w="100px" h="20px" bg="red"
                        Box w="50px" h="20px" bg="green"
                    }
                }
            } }
        },
        |cx| boxes = rects_of(cx, "box"),
    );

    assert_eq!(boxes.len(), 2, "both boxes exist: {boxes:?}");
    for r in &boxes {
        assert!(r.x >= 19.0, "inside the 20px padding: {r:?}");
        assert!(r.x + r.w <= 381.0, "and inside the viewport: {r:?}");
    }
    assert!((boxes[1].x - boxes[0].x - boxes[0].w - 8.0).abs() < 0.01, "the row gap applies");
}

#[test]
fn clipping_intersects_nested_bounds() {
    let outer = Rect::new(0.0, 0.0, 100.0, 100.0);
    let inner = Rect::new(80.0, 80.0, 100.0, 100.0);
    let both = outer.intersection(&inner);
    assert_eq!((both.w, both.h), (20.0, 20.0));

    // No overlap yields an empty rect rather than a negative size.
    let apart = outer.intersection(&Rect::new(500.0, 500.0, 10.0, 10.0));
    assert_eq!((apart.w, apart.h), (0.0, 0.0));
}

#[test]
fn a_clipped_container_passes_its_bounds_to_children() {
    let mut h = Harness::new();
    let mut clip = None;
    h.frame_with(
        viewport(),
        dark(),
        |_| {},
        |cx| {
            ui! { MiMUI {
                Column overflow="hidden" height="50px" {
                    Label "one"
                    Label "two"
                    Label "three"
                }
            } }
        },
        |cx| {
            // The last node is the third label, inside the clipping column.
            clip = cx.nodes()[cx.nodes().len() - 1].clip;
        },
    );
    let bounds = clip.expect("the last label inherits a clip");
    assert_eq!(bounds.h, 50.0, "children are clipped to the box");
}

// ---------------------------------------------------------------------------
// styling
// ---------------------------------------------------------------------------

#[test]
fn style_properties_reach_the_draw_list() {
    let mut h = Harness::new();
    let mut style = Style::default();
    h.frame_with(
        viewport(),
        dark(),
        |_| {},
        |cx| {
            ui! { MiMUI {
                Box w="50px" h="20px" bg="#ff0000" radius="6px"
            } }
        },
        |cx| style = cx.nodes()[1].resolved.clone(),
    );

    assert_eq!(style.background, Some(Color::rgb(255, 0, 0, 1.0)));
    assert_eq!(style.radius.tl, 6.0);
    assert_eq!(style.width, Dim::Px(50.0));
}

#[test]
fn a_class_applies_its_rule() {
    let mut h = Harness::new();
    let mut size = None;
    let frame = h.frame_with(
        viewport(),
        dark(),
        |cx| cx.add_class("wide", "width: 120px; height: 30px; background: #00ff00;"),
        |cx| {
            ui! { MiMUI {
                Box class="wide"
            } }
        },
        |cx| size = cx.nodes().get(1).map(|n| (n.rect.w, n.rect.h)),
    );

    assert_eq!(size, Some((120.0, 30.0)));
    assert!(!frame.cmds.is_empty());
}

#[test]
fn several_classes_combine_with_the_last_winning() {
    let mut h = Harness::new();
    let mut bg = None;
    h.frame_with(
        viewport(),
        dark(),
        |cx| {
            cx.add_class("card", "background: #ff0000; height: 10px;");
            cx.add_class("wide", "background: #00ff00;");
        },
        |cx| {
            ui! { MiMUI {
                Box class="card wide"
            } }
        },
        |cx| bg = cx.nodes()[1].resolved.background,
    );

    assert_eq!(bg, Some(Color::rgb(0, 255, 0, 1.0)), "the later class wins");
}

#[test]
fn inline_css_overrides_the_class() {
    let mut h = Harness::new();
    let mut height = None;
    h.frame_with(
        viewport(),
        dark(),
        |cx| cx.add_class("card", "height: 10px; background: #ff0000;"),
        |cx| {
            ui! { MiMUI {
                Box class="card" style="height: 40px;"
            } }
        },
        |cx| height = cx.nodes().get(1).map(|n| n.rect.h),
    );
    assert_eq!(height, Some(40.0), "an inline block wins over the class");
}

#[test]
fn glyphs_carry_the_node_color() {
    let mut h = Harness::new();
    let frame = h.frame(
        viewport(),
        Style::parse("background: #000000; color: #ff8800;"),
        |cx| {
            ui! { MiMUI {
                Column { Label "tinted" color="#ff8800" }
            } }
        },
    );
    let tinted = frame.cmds.iter().any(|c| {
        matches!(c, DrawCmd::Image { texture: TextureRef::GlyphAtlas, tint, .. }
            if tint.r > 0.9 && tint.g > 0.4 && tint.b < 0.2)
    });
    assert!(tinted, "the glyph tint follows `color`");
}

// ---------------------------------------------------------------------------
// interaction
// ---------------------------------------------------------------------------

#[test]
fn on_hover_styles_apply_when_the_pointer_arrives() {
    let mut h = Harness::new();
    let body = |cx: &mut UiCtx<'_>| {
        ui! { MiMUI {
            Column padding="10px" align="start" {
                Button id="b" "Hover"
                    style="background: #111111;"
                    on_hover="background: #ff0000;"
            }
        } }
    };

    // Far away: the base style applies.
    h.input.mouse = Vec2::new(380.0, 280.0);
    let away = h.frame(viewport(), dark(), body);
    assert_eq!(red_boxes(&away), 0, "not hovered yet");

    // Move onto the button, then check the following frame.
    let mut centre = Vec2::ZERO;
    h.frame_with(viewport(), dark(), |_| {}, body, |cx| {
        let b = cx.find_by_tag("button").expect("button");
        centre = cx.nodes()[b].rect.center();
    });
    h.input.mouse = centre;

    let over = h.frame(viewport(), dark(), body);
    assert!(red_boxes(&over) >= 1, "the hovered button should be red");
}

#[test]
fn clicking_a_button_pushes_its_link() {
    let mut h = Harness::new();
    let body = |cx: &mut UiCtx<'_>| {
        ui! { MiMUI {
            Column padding="10px" align="start" {
                Button id="b" "Go" link="https://example.com"
            }
        } }
    };

    let mut centre = Vec2::ZERO;
    h.frame_with(viewport(), dark(), |_| {}, body, |cx| {
        let b = cx.find_by_tag("button").expect("button");
        centre = cx.nodes()[b].rect.center();
    });
    h.input.mouse = centre;
    h.input.press_button(MouseButton::Left);
    h.frame(viewport(), dark(), body);
    h.input.release_button(MouseButton::Left);
    h.frame(viewport(), dark(), body);

    assert_eq!(
        h.state.take_links(),
        vec!["https://example.com".to_string()],
        "the link should be queued for the app"
    );
}

/// A click is press *and* release; activating on both would fire twice.
#[test]
fn one_click_activates_once() {
    let mut h = Harness::new();
    let body = |cx: &mut UiCtx<'_>| {
        ui! { MiMUI {
            Column padding="10px" align="start" {
                Checkbox id="a" label="Agree"
            }
        } }
    };

    let mut centre = Vec2::ZERO;
    h.frame_with(viewport(), dark(), |_| {}, body, |cx| {
        centre = cx.nodes()[cx.find_by_tag("checkbox-box").expect("checkbox")].rect.center();
    });

    h.input.mouse = centre;
    h.input.press_button(MouseButton::Left);
    h.frame(viewport(), dark(), body);
    h.input.release_button(MouseButton::Left);
    h.frame(viewport(), dark(), body);

    assert!(h.state.get_bool("a", false), "one click toggles once, not twice");

    // With the box ticked, the label still has to sit beside it.
    let mut label_right = 0.0f32;
    let mut box_right = 0.0f32;
    h.frame_with(viewport(), dark(), |_| {}, body, |cx| {
        let b = cx.find_by_tag("checkbox-box").expect("box");
        let l = cx.find_by_tag("label").expect("label");
        box_right = cx.nodes()[b].rect.max().x;
        label_right = cx.nodes()[l].rect.x;
    });
    assert!(label_right > box_right, "the tick did not swallow the label");
}

/// Dragging off a button before releasing cancels it.
#[test]
fn releasing_away_from_a_button_cancels_the_click() {
    let mut h = Harness::new();
    let body = |cx: &mut UiCtx<'_>| {
        ui! { MiMUI {
            Column padding="10px" align="start" {
                Button id="b" "Go" link="https://example.com"
            }
        } }
    };

    let mut centre = Vec2::ZERO;
    h.frame_with(viewport(), dark(), |_| {}, body, |cx| {
        centre = cx.nodes()[cx.find_by_tag("button").expect("button")].rect.center();
    });

    h.input.mouse = centre;
    h.input.press_button(MouseButton::Left);
    h.frame(viewport(), dark(), body);
    h.input.mouse = Vec2::new(380.0, 280.0);
    h.input.release_button(MouseButton::Left);
    h.frame(viewport(), dark(), body);

    assert!(h.state.take_links().is_empty(), "the click was cancelled");
}

/// Widgets that are built from several nodes have to keep them siblings.
/// If the wrapper stays open the second part lands *inside* the first, which
/// collapses the layout instead of just looking odd.
#[test]
fn compound_widgets_keep_their_parts_as_siblings() {
    let mut h = Harness::new();
    let parent_of = |cx: &UiCtx<'_>, tag: &str| -> Option<usize> {
        let i = cx.find_by_tag(tag)?;
        cx.nodes()[i].parent
    };
    let mut ok = false;
    h.frame_with(
        viewport(),
        dark(),
        |_| {},
        |cx| {
            ui! { MiMUI {
                Column padding="10px" align="stretch" {
                    Checkbox id="a" label="Agree"
                    Slider id="b"
                }
            } }
        },
        |cx| {
            let check = cx.find_by_tag("checkbox").expect("the checkbox wrapper");
            assert_eq!(cx.nodes()[check].children.len(), 2, "box + label");
            assert_eq!(parent_of(cx, "checkbox-box"), Some(check));
            assert_eq!(parent_of(cx, "label"), Some(check));

            let slider = cx.find_by_tag("slider").expect("the slider");
            assert_eq!(cx.nodes()[slider].children.len(), 3, "track + fill + knob");
            for part in ["slider-track", "slider-fill", "slider-knob"] {
                assert_eq!(parent_of(cx, part), Some(slider), "{part}");
            }
            ok = true;
        },
    );
    assert!(ok);
}

#[test]
fn a_checkbox_draws_its_tick_only_when_on() {
    let mut h = Harness::new();
    let off = h.frame(viewport(), dark(), |cx| {
        ui! { MiMUI { Column { Checkbox id="a" label="Agree" } } }
    });
    assert_eq!(tick_bars(&off), 0, "unchecked has no tick");

    h.state.set_bool("a", true);
    let on = h.frame(viewport(), dark(), |cx| {
        ui! { MiMUI { Column { Checkbox id="a" label="Agree" } } }
    });
    assert_eq!(tick_bars(&on), 2, "the tick is two bars");
}

/// White bars inside a checkbox box.
fn tick_bars(frame: &Frame) -> usize {
    frame
        .cmds
        .iter()
        .filter(|c| {
            matches!(c, DrawCmd::Box { rect, fill: Some(f), .. }
                if rect.w <= 3.0 && rect.h >= 4.0 && f.r > 0.95 && f.g > 0.95 && f.b > 0.95)
        })
        .count()
}

#[test]
fn a_checkbox_toggles_its_value() {
    let mut h = Harness::new();
    let body = |cx: &mut UiCtx<'_>| {
        ui! { MiMUI {
            Column padding="10px" align="start" {
                Checkbox id="agree" label="Agree"
            }
        } }
    };

    let mut centre = Vec2::ZERO;
    h.frame_with(viewport(), dark(), |_| {}, body, |cx| {
        let b = cx.find_by_tag("checkbox-box").expect("checkbox");
        centre = cx.nodes()[b].rect.center();
    });
    assert_eq!(h.state.get_bool("agree", false), false);

    h.input.mouse = centre;
    h.input.press_button(MouseButton::Left);
    h.frame(viewport(), dark(), body);
    h.input.release_button(MouseButton::Left);
    h.frame(viewport(), dark(), body);

    assert!(h.state.get_bool("agree", false), "the checkbox should be on");
}

#[test]
fn a_slider_follows_the_pointer() {
    let mut h = Harness::new();
    let body = |cx: &mut UiCtx<'_>| {
        ui! { MiMUI {
            Column padding="10px" align="stretch" {
                Slider id="vol"
            }
        } }
    };

    let mut press = Vec2::ZERO;
    h.frame_with(viewport(), dark(), |_| {}, body, |cx| {
        let s = cx.find_by_tag("slider").expect("slider");
        let r = cx.nodes()[s].rect;
        press = Vec2::new(r.x + r.w * 0.75, r.center().y);
    });

    h.input.mouse = press;
    h.input.press_button(MouseButton::Left);
    h.frame(viewport(), dark(), body);

    let v = h.state.get_num("vol", -1.0);
    assert!((v - 0.75).abs() < 0.02, "expected ~0.75, got {v}");
}

#[test]
fn a_text_input_shows_its_stored_value() {
    let mut h = Harness::new();
    h.state.set_text("name", "MiMUI");

    let mut seen = None;
    h.frame_with(
        viewport(),
        dark(),
        |_| {},
        |cx| {
            ui! { MiMUI {
                Column padding="10px" align="start" {
                    TextInput id="name" placeholder="type…"
                }
            } }
        },
        |cx| seen = text_of(cx, "text-input"),
    );

    let (text, placeholder) = seen.expect("the input has text");
    assert_eq!(text, "MiMUI", "the stored value is displayed");
    assert!(!placeholder, "a real value is not placeholder text");
}

#[test]
fn an_empty_text_input_shows_its_placeholder() {
    let mut h = Harness::new();
    let mut seen = None;
    h.frame_with(
        viewport(),
        dark(),
        |_| {},
        |cx| {
            ui! { MiMUI {
                Column padding="10px" align="start" {
                    TextInput id="name" placeholder="type…"
                }
            } }
        },
        |cx| seen = text_of(cx, "text-input"),
    );

    let (_, placeholder) = seen.expect("the input has text");
    assert!(placeholder, "no value means placeholder text");
}

// ---------------------------------------------------------------------------
// motion
// ---------------------------------------------------------------------------

#[test]
fn a_keyframe_animation_runs_and_settles() {
    anim!("_test_drop", [
        0.0 => { y: 40.0, opacity: 0.0 },
        1.0 => { y: 0.0, opacity: 1.0 },
    ]);

    let mut h = Harness::new();
    let mut alphas = Vec::new();
    for _ in 0..40 {
        h.run(
            1.0 / 30.0,
            viewport(),
            dark(),
            |_| {},
            |cx| {
                ui! { MiMUI {
                    Box w="10px" h="10px" bg="#ffffff" anim="_test_drop 0.4s linear"
                } }
            },
            |cx| alphas.push(cx.nodes()[1].resolved.opacity),
        );
    }

    assert_eq!(alphas.len(), 40);
    assert!(alphas[0] < 0.35, "starts nearly transparent, got {}", alphas[0]);
    assert!(
        (alphas[39] - 1.0).abs() < 0.02,
        "finishes opaque, got {}",
        alphas[39]
    );
    assert!(
        alphas.windows(2).take(6).all(|w| w[1] >= w[0]),
        "ramps up: {alphas:?}"
    );
}

#[test]
fn a_custom_animation_drives_a_transform() {
    // Registered at test time, exactly as an app would at startup.
    let name = anim!("_test_bounce", [
        0.0 => { scale: 0.5 },
        1.0 => { scale: 1.0 },
    ]);
    assert!(mimui::anim::get(name).is_some());

    let mut h = Harness::new();
    let mut scales = Vec::new();
    for _ in 0..20 {
        h.run(
            1.0 / 30.0,
            viewport(),
            dark(),
            |_| {},
            |cx| {
                ui! { MiMUI {
                    Box w="10px" h="10px" bg="red" anim="_test_bounce 0.4s linear"
                } }
            },
            |cx| scales.push(cx.nodes()[1].resolved.scale.x),
        );
    }

    assert!(scales[0] < 0.6, "starts small, got {}", scales[0]);
    assert!(
        (scales[19] - 1.0).abs() < 0.02,
        "finishes at full size, got {}",
        scales[19]
    );
}

#[test]
fn a_transition_interpolates_a_change() {
    let mut h = Harness::new();
    let mut samples = Vec::new();

    // Park the cursor off the box, then move onto it and sample the ramp.
    h.input.mouse = Vec2::new(380.0, 280.0);
    let body = |cx: &mut UiCtx<'_>| {
        ui! { MiMUI {
            Column padding="0px" align="start" {
                Box w="40px" h="40px" bg="#000000"
                    transition="background 0.2s linear"
                    on_hover="background: #ffffff;"
            }
        } }
    };

    for step in 0..20 {
        if step == 2 {
            h.input.mouse = Vec2::new(20.0, 20.0);
        }
        let mut red = 0.0f32;
        h.run(
            1.0 / 60.0,
            viewport(),
            dark(),
            |_| {},
            body,
            |cx| {
                let b = cx.find_by_tag("box").expect("the box");
                red = cx.nodes()[b].resolved.background.map_or(0.0, |c| c.r);
            },
        );
        samples.push(red);
    }

    assert!(samples[0] < 0.05, "starts black: {samples:?}");
    assert!(samples[19] > 0.9, "finishes white: {samples:?}");
    let middle = &samples[3..8];
    assert!(
        middle.iter().all(|v| *v > 0.0 && *v < 1.0),
        "intermediate values are partway: {samples:?}"
    );
    assert!(
        middle.windows(2).all(|w| w[1] > w[0]),
        "the transition ramps up: {samples:?}"
    );
}

#[test]
fn the_frame_asks_for_another_one_while_an_animation_runs() {
    anim!("_test_forever", [0.0 => { opacity: 1.0 }, 1.0 => { opacity: 1.0 }]);
    let mut h = Harness::new();
    let frame = h.frame(viewport(), dark(), |cx| {
        ui! { MiMUI {
            Column { Box w="10px" h="10px" bg="red" anim="_test_forever 1s infinite" }
        } }
    });
    assert!(frame.needs_redraw, "a running animation keeps the loop alive");
}

#[test]
fn a_still_frame_does_not_ask_for_another() {
    let mut h = Harness::new();
    let frame = h.frame(viewport(), dark(), |cx| {
        ui! { MiMUI {
            Column { Label "still" }
        } }
    });
    assert!(!frame.needs_redraw, "an idle UI should not spin the CPU");
}

#[test]
fn anim_props_read_and_write_the_style() {
    let mut s = Style::parse("radius: 4px; opacity: 0.5;");
    assert_eq!(AnimProp::Radius.read(&s), AnimVal::Num(4.0));
    assert_eq!(AnimProp::Opacity.read(&s), AnimVal::Num(0.5));

    AnimProp::Opacity.write(&mut s, AnimVal::Num(1.0));
    assert_eq!(s.opacity, 1.0);

    AnimProp::Radius.write(&mut s, AnimVal::Num(9.0));
    assert_eq!(s.radius.tl, 9.0);
}

// ---------------------------------------------------------------------------
// state that has to survive the tree being rebuilt
// ---------------------------------------------------------------------------

/// Every frame builds a brand-new node tree, so anything the next frame needs —
/// positions, resolved styles, running animations, in-flight transitions — has to
/// be looked up by `id` rather than carried on the node.
#[test]
fn hover_needs_the_previous_frames_rectangle() {
    let mut h = Harness::new();
    let body = |cx: &mut UiCtx<'_>| {
        ui! { MiMUI {
            Column padding="10px" align="start" {
                Box w="40px" h="40px" bg="#000000"
                    on_hover="background: #ff0000;"
            }
        } }
    };

    // Far away: nothing.
    h.input.mouse = Vec2::new(380.0, 280.0);
    assert_eq!(red_boxes(&h.frame(viewport(), dark(), body)), 0);

    // On the box. The first frame after the move still uses last frame's rect.
    h.input.mouse = Vec2::new(30.0, 30.0);
    h.frame(viewport(), dark(), body);
    let over = h.frame(viewport(), dark(), body);
    assert_eq!(red_boxes(&over), 1, "hover takes effect on the following frame");
}

#[test]
fn a_state_block_does_not_leak_into_the_base_style() {
    let mut h = Harness::new();
    let mut base = Style::default();
    let mut hovered = Style::default();
    h.frame_with(
        viewport(),
        dark(),
        |_| {},
        |cx| {
            ui! { MiMUI {
                Column {
                    Box w="40px" h="40px" bg="#000000"
                        transition="background 0.2s, scale 0.15s"
                        on_hover="background: #ffffff; scale: 1.04;"
                }
            } }
        },
        |cx| {
            let b = cx.find_by_tag("box").expect("the box");
            base = cx.nodes()[b].resolved.clone();
            hovered = cx.nodes()[b].styles.hover.clone().expect("a hover block");
        },
    );

    // `on_hover` is a declaration block, not a property: reading it as one
    // would splice `scale: 1.04` into the always-on style.
    assert_eq!(base.scale, Vec2::ONE, "the base is unscaled");
    assert_eq!(base.background, Some(Color::rgb(0, 0, 0, 1.0)));
    assert_eq!(hovered.scale, Vec2::splat(1.04), "the hover block does scale");
}

#[test]
fn a_state_block_keeps_the_widgets_padding() {
    let mut h = Harness::new();
    let mut plain = Rect::ZERO;
    let mut hovered = Rect::ZERO;
    let body = |cx: &mut UiCtx<'_>| {
        ui! { MiMUI {
            Column padding="10px" align="start" {
                Button id="b" "Hover"
                    style="background: #111111;"
                    on_hover="background: #ff0000;"
            }
        } }
    };

    h.input.mouse = Vec2::new(380.0, 280.0);
    h.frame_with(viewport(), dark(), |_| {}, body, |cx| {
        let b = cx.find_by_tag("button").unwrap();
        plain = cx.nodes()[b].rect;
    });
    h.input.mouse = plain.center();
    h.frame(viewport(), dark(), body);
    h.frame_with(viewport(), dark(), |_| {}, body, |cx| {
        let b = cx.find_by_tag("button").unwrap();
        hovered = cx.nodes()[b].rect;
    });

    assert!(
        hovered.w > 20.0 && hovered.h > 20.0,
        "`on_hover` must only override the colour, not drop the padding: {hovered:?}"
    );
}

#[test]
fn tab_moves_focus_through_the_widgets() {
    let mut h = Harness::new();
    let body = |cx: &mut UiCtx<'_>| {
        ui! { MiMUI {
            Column padding="10px" align="start" {
                Button id="one" "One"
                Button id="two" "Two"
            }
        } }
    };

    h.frame(viewport(), dark(), body);
    assert_eq!(h.state.focus, None, "nothing is focused to begin with");

    h.input.press_key(Key::Tab);
    h.frame(viewport(), dark(), body);
    assert!(h.state.focus.is_some(), "tab focuses the first widget");

    h.input.press_key(Key::Tab);
    h.frame(viewport(), dark(), body);
    assert_eq!(h.state.focus.as_deref(), Some("two"), "tab advances");
}

#[test]
fn enter_activates_the_focused_widget() {
    let mut h = Harness::new();
    let body = |cx: &mut UiCtx<'_>| {
        ui! { MiMUI {
            Column padding="10px" align="start" {
                Checkbox id="agree" label="Agree"
            }
        } }
    };

    h.state.focus = Some("agree".to_string());
    h.input.press_key(Key::Enter);
    h.frame(viewport(), dark(), body);

    assert!(h.state.get_bool("agree", false), "enter toggles the focused checkbox");
}

// ---------------------------------------------------------------------------
// visibility and images
// ---------------------------------------------------------------------------

#[test]
fn display_none_draws_nothing() {
    let mut h = Harness::new();
    let frame = h.frame(viewport(), dark(), |cx| {
        ui! { MiMUI {
            Column { Box w="20px" h="20px" bg="red" display="none" }
        } }
    });
    assert_eq!(red_boxes(&frame), 0, "display:none should draw nothing");
}

#[test]
fn zero_opacity_draws_nothing() {
    let mut h = Harness::new();
    let frame = h.frame(viewport(), dark(), |cx| {
        ui! { MiMUI {
            Column { Box w="20px" h="20px" bg="red" opacity="0" }
        } }
    });
    assert_eq!(red_boxes(&frame), 0, "a fully transparent node should draw nothing");
}

#[test]
fn display_none_removes_the_whole_subtree() {
    let mut h = Harness::new();
    let frame = h.frame(viewport(), dark(), |cx| {
        ui! { MiMUI {
            Column { Box bg="red" display="none" { Label "hidden" } }
        } }
    });
    let glyphs = frame
        .cmds
        .iter()
        .filter(|c| matches!(c, DrawCmd::Image { texture: TextureRef::GlyphAtlas, .. }))
        .count();
    assert_eq!(glyphs, 0, "children of a hidden box stay hidden");
}

#[test]
fn an_image_is_reported_as_needing_a_load() {
    let mut h = Harness::new();
    let frame = h.frame(viewport(), dark(), |cx| {
        ui! { MiMUI {
            Column { Image src="missing.png" w="40px" h="40px" }
        } }
    });

    assert_eq!(frame.missing.len(), 1, "the renderer is asked to load it");
    assert_eq!(frame.missing[0].source, "missing.png");
    // A draw command is still emitted; the renderer skips it until uploaded.
    assert!(
        frame
            .cmds
            .iter()
            .any(|c| matches!(c, DrawCmd::Image { texture: TextureRef::Image(_), .. })),
        "the image should appear in the draw list"
    );
}

// ---------------------------------------------------------------------------
// the exact syntax from the README
// ---------------------------------------------------------------------------

#[test]
fn the_readme_snippet_renders() {
    let mut h = Harness::new();
    let mut tags = Vec::new();
    h.frame_with(
        viewport(),
        dark(),
        |_| {},
        |cx| {
            ui! { MiMUI {
                Label "MiMUI";
                Button "Click!" link="openoptionswindow";
                Button class="bottom-btn" "github" link="github.com";
            } }
        },
        |cx| tags = (0..cx.nodes().len()).map(|i| cx.nodes()[i].tag.clone()).collect(),
    );

    assert_eq!(tags, vec!["mimui", "label", "button", "button"]);
    assert_eq!(h.state.take_links(), Vec::<String>::new());
}