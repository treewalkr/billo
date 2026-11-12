CREATE TABLE users (
    id          UUID PRIMARY KEY,
    google_sub  TEXT UNIQUE NOT NULL,
    email       TEXT NOT NULL,
    locale      TEXT NOT NULL DEFAULT 'th',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
