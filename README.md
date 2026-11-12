# Billo

Personal finance app for tracking income, expenses, credit cards, installments and loans. Built for Thailand first (Thai/English, THB). Runs on web, iOS and Android.

> **Status:** early development. Domain logic and a server skeleton exist; no app yet.

## Stack

All Rust, one monorepo:

- **App:** Dioxus (web, iOS, Android)
- **Server:** Axum + PostgreSQL
- **Shared:** `billo-domain`, the financial calculations used by both the app and the server

## Run locally

Requires Docker and a Rust toolchain.

```bash
docker compose up -d                      # Postgres + server on :8080
curl localhost:8080/api/v1/health         # {"status":"ok","version":"0.1.0","build":1}
```

Tests (database tests are skipped if `DATABASE_URL` is unset):

```bash
docker compose up -d postgres
cp .env.example .env                      # optional, for `cargo run -p billo-server`
DATABASE_URL=postgres://billo:billo@localhost:5432/billo cargo test
```

## Docs

- [PRODUCT.md](PRODUCT.md): what we're building and why
- [docs/architecture.md](docs/architecture.md): how it's built

## License

TBD
