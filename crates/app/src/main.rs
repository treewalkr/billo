//! Billo app: one Dioxus codebase for web, iOS and Android.

mod api;
mod demo;
mod format;
mod ui;

fn main() {
    dioxus::launch(ui::App);
}
