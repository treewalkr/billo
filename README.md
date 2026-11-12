# Billo

Personal finance app for tracking income, expenses, credit cards, installments and loans. Built for Thailand first (Thai/English, THB). Runs on web, iOS and Android.

> **Status:** early planning. No code yet.

## Stack

All Rust, one monorepo:

- **App:** Dioxus (web, iOS, Android)
- **Server:** Axum + PostgreSQL
- **Shared:** `billo-domain`, the financial calculations used by both the app and the server

## Docs

- [PRODUCT.md](PRODUCT.md): what we're building and why
- [docs/architecture.md](docs/architecture.md): how it's built

## License

TBD
