//! Billo app: one Dioxus codebase for web, iOS and Android.

mod api;
mod budgets;
mod categories;
mod clock;
mod convert;
mod format;
mod forms;
mod input;
mod overview;
mod storage;
mod strings;
mod ui;

fn main() {
    dioxus::launch(ui::App);
}
