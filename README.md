# Billo

Personal finance app for tracking income, expenses, credit cards, installments and loans. Built for Thailand first (Thai/English, THB). Runs on web, iOS and Android.

> **Status:** early development. Domain logic, a server skeleton and a first app screen (sample data) exist.

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

Google sign-in isn't wired up yet. For local development the compose file sets `DEV_LOGIN=true`, which lets you sign in with just an email (never enable it in production):

```bash
curl -X POST localhost:8080/api/v1/auth/dev-login \
  -H 'content-type: application/json' -d '{"email":"me@example.com"}'
# {"token":"…","user_id":"…"}  → send as `Authorization: Bearer <token>`
curl localhost:8080/api/v1/accounts -H "authorization: Bearer $TOKEN"
```

Web app (needs the [Dioxus CLI](https://dioxuslabs.com/learn/0.7/getting_started): `cargo install dioxus-cli --version 0.7.10 --locked`):

```bash
docker compose up -d                      # the server allows CORS from :8081 in dev
cd crates/app && dx serve --platform web --port 8081
# open http://localhost:8081
```

The app talks to `http://localhost:8080` by default; set `BILLO_API_URL` at build time to change it.

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
