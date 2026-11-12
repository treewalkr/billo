//! Billo app: one Dioxus codebase for web, iOS and Android.

// The full client lands first; the screens adopt it in the next commits.
#[allow(dead_code)]
mod api;
mod clock;
mod convert;
mod demo;
mod format;
mod forms;
mod input;
mod storage;
mod strings;
mod ui;

fn main() {
    dioxus::launch(ui::App);
}
