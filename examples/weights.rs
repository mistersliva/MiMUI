//! Renders the same string at several font weights, to check that the font
//! stack resolves to a face for each one.

use mimui::prelude::*;

struct Weights;

impl App for Weights {
    fn ui(&mut self, cx: &mut UiCtx<'_>) {
        ui! { MiMUI {
            Column gap="12px" padding="24px" align="start" {
                Label "weight 400: Hamburgefonstiv" font-weight=400
                Label "weight 500: Hamburgefonstiv" font-weight=500
                Label "weight 600: Hamburgefonstiv" font-weight=600
                Label "weight 700: Hamburgefonstiv" font-weight=700
                Button "button default weight"
                Label "numeric 500: 0123456789" font-weight=500
            }
        } }
    }
}

fn main() {
    mimui::run(Weights, WindowOptions::default().title("weights").size(640, 320));
}