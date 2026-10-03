// Scratch: the snippets from docs/extending.md and docs/reference.md must
// actually compile against the crate.
use mimui::input::InputState;
use mimui::prelude::*;
use mimui::text::TextEngine;

// --- reference.md: elements, aliases, state blocks -------------------------
#[allow(dead_code)]
fn elements(cx: &mut UiCtx<'_>) {
    ui! { MiMUI {
        Column direction="row" {
            Row gap="8px" wrap="wrap" {
                Box class="button button-primary" { Label "x" }
                Title "t"
                Label text="hi"
                Button "Save" link="https://example.com"
                Image src="a.png" fit="contain" tint="#fff"
                Progress value=0.5 color="#4a7fff"
                Checkbox id="c" label="Agree" checked color="#4a7fff"
                Slider id="s" value=0.5
                TextInput id="t" placeholder="p" value="v"
                Spacer
                Divider
            }
        }
    } }
    let _ = cx;
}

// --- reference.md: state block spellings ------------------------------------
#[allow(dead_code)]
fn state_blocks(cx: &mut UiCtx<'_>) {
    ui! { MiMUI {
        Box
            on-hover="background: red;"
            on_active="background: blue;"
            on-focus="background: green;"
            disabled="opacity: 0.5;"
            hover="background: red;"
            active="background: blue;"
            focus="background: green;"
    } }
    let _ = cx;
}

// --- reference.md: property values -----------------------------------------
#[allow(dead_code)]
fn properties(cx: &mut UiCtx<'_>) {
    ui! { MiMUI {
        Column
            display="flex" position="relative" overflow="hidden" visible=true z-index="2"
            direction="column" gap="10px" align="center" justify="space-between"
            align-self="end" justify-self="center"
            flex="1 1 0" flex-grow="1" flex-shrink="0" flex-basis="0"
            w="50%" h="auto" size="40px 24px" min-width="10px" max-width="90%"
            padding="4px 8px" margin="2px" inset="0" gap-x="6px"
            bg="#ff6b6b" background="#ff6b6b" background-color="rgba(0,0,0,0.5)"
            image="a.png" background-size="cover" color="white" fg="#eee" foreground="#eee" text-color="#eee"
            border="1px #2b3550" border-width="1px" border-color="#2b3550"
            radius="10px / 4px" border-radius="999px"
            shadow="0 6px 18px #0009" box-shadow="none" shadow-blur="4px" shadow-color="#0006"
            opacity="0.5" clip=true
            font-size="14px" font-family="Inter" font-weight="semibold" font-style="italic"
            line-height="1.4" letter-spacing="0.5px" text-align="center" word-wrap=true
            translate="4px 2px" translate-x="2px" translate-y="2px"
            scale="1.05" scale-x="1.05" scale-y="1.05" rotate="90"
            cursor="pointer" transition="background 0.2s" ease="ease-out"
            transition-duration="0.2s" transition-delay="0.1s"
    } }
    let _ = cx;
}

// --- reference.md: colour forms --------------------------------------------
#[allow(dead_code)]
fn colours(cx: &mut UiCtx<'_>) {
    ui! { MiMUI {
        Row {
            Box bg="#f00"         Box bg="#f00a"   Box bg="#ff0000"  Box bg="#ff0000aa"
            Box bg="rgb(1,2,3)"  Box bg="rgba(1,2,3,0.5)"
            Box bg="transparent"  Box bg="white"   Box bg="navy"
        }
    } }
    let _ = cx;
}

// --- reference.md: easings and anim shorthands -----------------------------
#[allow(dead_code)]
fn motion(cx: &mut UiCtx<'_>) {
    anim!("doc_a", [0.0 => { opacity: 0.0 }, 1.0 => { opacity: 1.0 }]);
    anim!("doc_b", [0.0 => { y: 8.0 }, 0.5 => { y: -4.0 }]);
    ui! { MiMUI {
        Column {
            Box anim="doc_a 0.5s 0.06s ease-out"
            Box anim="ease-out doc_a 0.5s 0.06s"
            Box anim="doc_b 1.2s infinite"
            Box anim="doc_b 0.3s 3 alternate"
            Box anim="doc_b 0.3s reverse"
            Box anim="pop 250ms"
            Box transition="background 0.2s, scale 0.15s 0.05s ease-out"
            Box transition="all 0.2s"
            Box transition="0.2s"
            Box transition="background 200ms linear"
        }
    } }
    let _ = cx;
}

// --- reference.md: built-in animation names --------------------------------
#[allow(dead_code)]
fn builtins(cx: &mut UiCtx<'_>) {
    ui! { MiMUI {
        Column {
            Box anim="fade_in 1s"   Box anim="fade_out 1s"  Box anim="pop 1s"
            Box anim="zoom_in 1s"   Box anim="zoom_out 1s"   Box anim="pulse 1s"
            Box anim="slide_up 1s"  Box anim="slide_down 1s"
            Box anim="slide_left 1s" Box anim="slide_right 1s"
            Box anim="shake 1s"     Box anim="float 1s"      Box anim="spin 1s"
            Box anim="wobble 1s"    Box anim="bounce_in 1s"  Box anim="swing 1s"
            Box anim="grow 1s"
        }
    } }
    let _ = cx;
}

// --- reference.md: aliases actually reach the same widgets ------------------
#[allow(dead_code)]
fn aliases(cx: &mut UiCtx<'_>) {
    ui! { MiMUI {
        VStack { HStack { Panel { Span "a" } } }
        Div { Node "b" }
        Btn "c"
        Img src="a.png"
        ProgressBar value=0.1
        Check label="d"
        Range id="r"
        Input id="i"
        Space
        Hr
    } }
    let _ = cx;
}

// --- reference.md: app + window options + state ----------------------------
#[allow(dead_code)]
struct App1;
impl App for App1 {
    fn ui(&mut self, cx: &mut UiCtx<'_>) {
        ui! { MiMUI { Label "x" } }
        let _: bool = cx.state().get_bool("b", false);
        let _: f32 = cx.state().get_num("n", 0.0);
        let _: String = cx.state().get_text("t", "");
        cx.state().set_bool("b", true);
        cx.state().set_num("n", 1.0);
        cx.state().set_text("t", "x");
        cx.state().set("b", Value::Bool(true));
        let _: Vec<String> = cx.state().take_links();
        let _: bool = cx.state().flag("sidebar");
        cx.state().set_flag("sidebar", true);
        let _: bool = cx.state().toggle_flag("sidebar");
        cx.state().quit = true;
        cx.request_redraw();
    }
    fn on_link(&mut self, _link: &str) {}
}

/// Built, not run: `mimui::run` needs a real event loop on the main thread,
/// which a test thread is not.
#[allow(dead_code)]
fn options() -> WindowOptions {
    WindowOptions::default()
        .title("t")
        .size(100, 100)
        .min_size(10, 10)
        .theme(Style::parse("background: #10131c;"))
        .stylesheet(".card { background: #1b2233; }")
        .font(Vec::new(), "Inter")
        .fps(60)
        .resizable(false)
}

#[test]
fn every_documented_spelling_compiles() {
    // The DSL forms are checked by the compiler; the runtime assertions below
    // only guard the parts that could silently change meaning.
    let mut input = InputState::new();
    let mut state = UiState::new();
    let mut text = TextEngine::new();
    let clock = Clock::default();
    let mut cx =
        UiCtx::new(&mut input, &mut state, &mut text, &clock, Vec2::new(400.0, 300.0), 1.0);
    cx.set_theme(Style::parse("background: #10131c; color: white;"));
    cx.add_class("button", "padding: 6px;");
    cx.add_class("button-primary", "background: #4a7fff;");
    cx.add_stylesheet(".card { background: #1b2233; }");

    elements(&mut cx);
    state_blocks(&mut cx);
    properties(&mut cx);
    motion(&mut cx);
    builtins(&mut cx);
    aliases(&mut cx);

    cx.resolve_styles(1.0 / 60.0);
    cx.measure_text();
    cx.layout(Rect::new(0.0, 0.0, 400.0, 300.0));
    cx.handle_interactions();
    let frame = cx.build_frame();
    assert!(!frame.cmds.is_empty(), "the documented forms draw something");

    let o = options();
    assert_eq!(o.title, "t");
    assert_eq!((o.width, o.height), (100, 100));
    assert!((o.min_width, o.min_height) == (10, 10));
    assert_eq!(o.fps, 60);
    assert!(!o.resizable);
}