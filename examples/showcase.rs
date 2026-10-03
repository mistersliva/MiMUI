//! The MiMUI showcase: every widget, styling technique and animation style.
//!
//! Run it with:
//!
//! ```sh
//! cargo run --example showcase
//! ```

use mimui::prelude::*;

const CARD_IN: &str = "card_in";
const GLOW: &str = "glow";

const STYLE_SHEET: &str = r#"
// Classes keep the ui! blocks readable. `.` makes a class.
.title {
    font-size: 30px;
    font-weight: 700;
    color: #eef2ff;
}

.subtitle {
    font-size: 14px;
    color: #8b96b4;
}

.bottom-btn {
    padding: 8px 14px;
    background: #232c44;
    border-width: 1px;
    border-color: #3a456a;
    radius: 8px;
}

.card {
    background: #1b2233;
    border-width: 1px;
    border-color: #2b3550;
    radius: 14px;
    padding: 14px 16px;
    gap: 10px;
}

.section {
    font-size: 12px;
    font-weight: 600;
    color: #6f7d9e;
    letter-spacing: 1px;
}

.value {
    font-size: 15px;
    color: #d7e0f5;
    font-weight: 500;
}
"#;

struct Showcase {
    clicks: u32,
    checked: bool,
    slider: f32,
    typed: String,
}

impl Showcase {
    fn new() -> Self {
        Self { clicks: 0, checked: false, slider: 0.6, typed: String::new() }
    }
}

/// Registers custom animations. Positions are fractions of the duration, so
/// `0.0` is the start and `1.0` the end; `0.5` is halfway.
fn register_animations() {
    anim!(CARD_IN, [
        0.0 => { y: 24.0, opacity: 0.0 },
        1.0 => { y: 0.0, opacity: 1.0 },
    ]);

    anim!(GLOW, [
        0.0 => { background: "#2a3550" },
        0.5 => { background: "#4a7fff" },
        1.0 => { background: "#2a3550" },
    ]);
}

impl App for Showcase {
    fn ui(&mut self, cx: &mut UiCtx<'_>) {
        ui! { MiMUI {
            Column gap="20px" padding="24px" {

                Label class="title" "MiMUI";
                Label class="subtitle" "immediate mode · CSS styling · declarative animation";

                Row gap="10px" {
                    Button "Count a click" link="count" anim="pop 0.25s";
                    Button class="primary" "github" link="https://github.com/mistersliva/MiMUI";
                    Progress value=0.72 color="#4a7fff"
                }

                Divider

                Row gap="12px" align="stretch" {
                    Column class="card" flex="1" anim="slide_up 0.35s ease-out" {
                        Label class="section" "BUILT-IN ANIMATIONS";
                        Label class="value" "fade_in · pop · pulse · shake";
                        Label class="value" "slide_up · bounce_in · spin";
                        Row gap="8px" {
                            Button "pop" anim="pop 0.3s";
                            Button "shake" anim="shake 0.5s";
                            Button "pulse" anim="pulse 1.2s infinite";
                        }
                    }

                    Column class="card" flex="1" anim="slide_up 0.35s 0.06s ease-out" {
                        Label class="section" "YOUR OWN";
                        Label class="value" (CARD_IN);
                        Row gap="8px" {
                            Button "card_in" anim="card_in 0.5s ease-out";
                            Button "glow" anim="glow 1.6s infinite"
                        }
                    }
                }

                Row gap="12px" align="stretch" {
                    Column class="card" flex="1" {
                        Label class="section" "INPUT";
                        Checkbox id="agree" label="Enable animations" checked
                        Slider id="vol" value=0.6
                        TextInput id="name" placeholder="type something…"
                        Row gap="8px" {
                            Label class="value" (self.typed.clone());
                            Label class="subtitle" "clicks:";
                            Label class="value" (self.clicks.to_string());
                        }
                    }

                    Column class="card" flex="1" {
                        Label class="section" "STYLING";
                        Label class="value" "1px solid #2b3550";
                        Row gap="8px" {
                            Box w="34px" h="34px" radius="10px" bg="#4a7fff"
                            Box w="34px" h="34px" radius="10px" bg="#39d98a"
                            Box w="34px" h="34px" radius="10px" bg="#ff6b6b"
                            Box w="34px" h="34px" radius="10px" bg="#ffd166"
                            Box w="34px" h="34px" radius="999px" bg="transparent"
                                border-width="2px" border-color="#ffd166"
                        }
                        Label class="value" "hover me:";
                        Box w="100%" h="40px" radius="10px" bg="#232c44"
                            transition="background 0.2s, scale 0.15s"
                            on_hover="background: #4a7fff; scale: 1.04;"
                            on_active="background: #2f6fd0;"
                        Label class="value" "shadow + gradient-free polish:";
                        Box w="100%" h="40px" radius="10px" bg="#2a3550"
                            shadow="0 6px 18px #0009"
                    }
                }

                Spacer

                Row gap="10px" {
                    Label class="subtitle" "bottom bar:";
                    Button class="bottom-btn" "options" link="openoptionswindow"
                    Button class="bottom-btn" "github" link="https://github.com/mistersliva/MiMUI"
                }
            }
        } }

        // Widget state is keyed by `id`, so the app reads it after building.
        self.checked = cx.state().get_bool("agree", false);
        self.slider = cx.state().get_num("vol", 0.6);
        self.typed = cx.state().get_text("name", "");
    }

    fn on_link(&mut self, link: &str) {
        match link {
            // Non-URL links are the app's own events.
            "count" => self.clicks += 1,
            "openoptionswindow" => {}
            // URLs open in the browser.
            other => mimui::open_in_browser(other),
        }
    }
}

fn main() {
    register_animations();
    mimui::run(
        Showcase::new(),
        WindowOptions::default()
            .title("MiMUI — showcase")
            .size(980, 780)
            .stylesheet(STYLE_SHEET),
    );
}