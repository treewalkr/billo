-- Web Google sign-in: one row per started login, consumed by the OAuth callback.
-- Short-lived (10 minutes) and single-use; only a hash of `state` is stored.
CREATE TABLE login_attempts (
    state_hash     BYTEA PRIMARY KEY CHECK (octet_length(state_hash) = 32),
    pkce_verifier  TEXT NOT NULL,
    nonce          TEXT NOT NULL,
    return_to      TEXT NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at     TIMESTAMPTZ NOT NULL
);
CREATE INDEX login_attempts_expires_at_idx ON login_attempts (expires_at);
