//! Billo app: one Dioxus codebase for web, iOS and Android.

mod api;
mod budgets;
mod categories;
mod clock;
mod convert;
mod format;
mod forms;
mod input;
// Wired into the screens in a later change.
#[allow(dead_code, unused_imports)]
mod local;
mod overview;
mod storage;
mod strings;
#[allow(dead_code)]
mod sync;
mod ui;

fn main() {
    dioxus::launch(ui::App);
}
