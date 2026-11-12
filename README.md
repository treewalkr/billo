# Billo

Personal finance app for tracking income, expenses, credit cards, installments and loans. Built for Thailand first (Thai/English, THB). Runs on web, iOS and Android.

> **Status:** early development. The web app works with real data entered by hand: accounts, card and loan settings, transactions, installment plans, categories, budgets and a "This month" view of what's due. It keeps your data on the device (IndexedDB on the web, SQLite on iOS / Android), works offline, and syncs with the server when it's back online.

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

### Google sign-in

The web app signs in with Google when the server has an OAuth client. Put these in the repo-root `.env` (gitignored; `docker compose` reads it, and `.env.example` lists them with placeholders). Never commit the real values:

- `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`: the **web** OAuth client. Google sign-in is on only when both are set.
- `GOOGLE_IOS_CLIENT_ID` (optional): the iOS client, also accepted by `POST /api/v1/auth/google`.
- `PUBLIC_URL` (default `http://localhost:8080`): where the browser reaches the server. The web client's authorized redirect URI must be exactly `{PUBLIC_URL}/api/v1/auth/google/callback`, and its authorized JavaScript origin `http://localhost:8081`.
- `APP_REDIRECT_ORIGINS` (default `http://localhost:8081` in compose): app origins the sign-in may return to.

```bash
docker compose up -d --build server
curl localhost:8080/api/v1/auth/methods   # {"google_sign_in":true,"dev_login":true}
```

While the Google consent screen is in Testing mode, only its listed test users can sign in. The iOS / Android apps don't offer Google sign-in yet.

### Dev login

For local development the compose file sets `DEV_LOGIN=true`, which lets you sign in with just an email (never enable it in production):

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
# open http://localhost:8081 and sign in with Google, or with any email (dev login)
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
