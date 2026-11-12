//! Billo app: one Dioxus codebase for web, iOS and Android.

mod api;
mod budgets;
mod categories;
mod clock;
mod convert;
mod format;
mod forms;
mod input;
mod local;
mod overview;
mod pwa;
mod storage;
mod store;
mod strings;
mod sync;
mod ui;

fn main() {
    dioxus::launch(ui::App);
}
