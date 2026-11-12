-- Core data: sessions, accounts, categories, transactions, installment plans, budgets
-- and the per-user change feed. Money is BIGINT minor units + ISO currency code; no
-- floating or NUMERIC types. Entity ids are client-generated UUIDv7s.

CREATE TABLE sessions (
    id            UUID PRIMARY KEY,
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- SHA-256 of the raw token; the token itself is never stored.
    token_hash    BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    device_label  TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at    TIMESTAMPTZ NOT NULL
);
CREATE INDEX sessions_user_id_idx ON sessions (user_id);

CREATE TABLE accounts (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('cash', 'bank', 'card', 'revolving', 'loan')),
    name        TEXT NOT NULL,
    currency    TEXT NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    -- Card / loan settings, versioned inside the JSON: {"v": 1, ...}.
    rules       JSONB NOT NULL DEFAULT '{"v": 1}',
    opened_on   DATE,
    closed_on   DATE,
    version     BIGINT NOT NULL DEFAULT 1,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMPTZ,
    -- Target for composite foreign keys, so rows can only reference the same user's rows.
    UNIQUE (user_id, id)
);
CREATE INDEX accounts_user_live_idx ON accounts (user_id) WHERE deleted_at IS NULL;

CREATE TABLE categories (
    id          UUID PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    parent_id   UUID,
    name_th     TEXT,
    name_en     TEXT,
    kind        TEXT NOT NULL CHECK (kind IN ('income', 'expense')),
    version     BIGINT NOT NULL DEFAULT 1,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMPTZ,
    UNIQUE (user_id, id),
    FOREIGN KEY (user_id, parent_id) REFERENCES categories (user_id, id),
    CHECK (name_th IS NOT NULL OR name_en IS NOT NULL),
    CHECK (parent_id IS DISTINCT FROM id)
);
CREATE INDEX categories_user_live_idx ON categories (user_id) WHERE deleted_at IS NULL;

CREATE TABLE installment_plans (
    id               UUID PRIMARY KEY,
    user_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    card_account_id  UUID NOT NULL,
    principal_minor  BIGINT NOT NULL CHECK (principal_minor > 0),
    currency         TEXT NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    months           INTEGER NOT NULL CHECK (months > 0),
    -- Flat monthly rate in basis points: 69 = 0.69% a month, 0 for 0% plans.
    monthly_rate_bp  INTEGER NOT NULL CHECK (monthly_rate_bp BETWEEN 0 AND 10000),
    first_post_on    DATE NOT NULL,
    closed_on        DATE,
    -- Rounding of each portion: step in minor units and mode.
    rounding_step    BIGINT NOT NULL DEFAULT 1 CHECK (rounding_step > 0),
    rounding_mode    TEXT NOT NULL DEFAULT 'half_up' CHECK (rounding_mode IN ('up', 'down', 'half_up')),
    remainder        TEXT NOT NULL DEFAULT 'last' CHECK (remainder IN ('first', 'last')),
    version          BIGINT NOT NULL DEFAULT 1,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at       TIMESTAMPTZ,
    UNIQUE (user_id, id),
    FOREIGN KEY (user_id, card_account_id) REFERENCES accounts (user_id, id)
);
CREATE INDEX installment_plans_user_live_idx ON installment_plans (user_id) WHERE deleted_at IS NULL;
CREATE INDEX installment_plans_card_idx ON installment_plans (user_id, card_account_id);

CREATE TABLE transactions (
    id                   UUID PRIMARY KEY,
    user_id              UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    account_id           UUID NOT NULL,
    category_id          UUID,
    occurred_on          DATE NOT NULL,
    posted_on            DATE,
    amount_minor         BIGINT NOT NULL,
    currency             TEXT NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    -- Original amount for foreign-currency spending.
    orig_amount_minor    BIGINT,
    orig_currency        TEXT CHECK (orig_currency ~ '^[A-Z]{3}$'),
    kind                 TEXT NOT NULL CHECK (kind IN ('expense', 'income', 'transfer', 'payment', 'fee', 'interest_adjust')),
    -- The other leg of a transfer; not a foreign key so either leg can be written first.
    transfer_peer_id     UUID,
    installment_plan_id  UUID,
    note                 TEXT,
    version              BIGINT NOT NULL DEFAULT 1,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at           TIMESTAMPTZ,
    FOREIGN KEY (user_id, account_id) REFERENCES accounts (user_id, id),
    FOREIGN KEY (user_id, category_id) REFERENCES categories (user_id, id),
    FOREIGN KEY (user_id, installment_plan_id) REFERENCES installment_plans (user_id, id),
    CHECK ((orig_amount_minor IS NULL) = (orig_currency IS NULL))
);
CREATE INDEX transactions_user_date_idx ON transactions (user_id, occurred_on DESC) WHERE deleted_at IS NULL;
CREATE INDEX transactions_user_account_date_idx ON transactions (user_id, account_id, occurred_on DESC) WHERE deleted_at IS NULL;
CREATE INDEX transactions_user_category_idx ON transactions (user_id, category_id);
CREATE INDEX transactions_user_plan_idx ON transactions (user_id, installment_plan_id);

CREATE TABLE budgets (
    id            UUID PRIMARY KEY,
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    category_id   UUID NOT NULL,
    -- First day of the budgeted month.
    period_month  DATE NOT NULL CHECK (extract(day FROM period_month) = 1),
    limit_minor   BIGINT NOT NULL CHECK (limit_minor >= 0),
    currency      TEXT NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    version       BIGINT NOT NULL DEFAULT 1,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at    TIMESTAMPTZ,
    FOREIGN KEY (user_id, category_id) REFERENCES categories (user_id, id)
);
-- One live budget per category and month.
CREATE UNIQUE INDEX budgets_user_category_month_idx
    ON budgets (user_id, category_id, period_month) WHERE deleted_at IS NULL;
CREATE INDEX budgets_user_month_idx ON budgets (user_id, period_month) WHERE deleted_at IS NULL;

-- Append-only per-user change feed; one row per create/update/delete, written in the
-- same transaction as the change. Feeds `GET /api/v1/sync/pull?since=<seq>`.
CREATE TABLE change_log (
    seq        BIGSERIAL PRIMARY KEY,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    entity     TEXT NOT NULL CHECK (entity IN ('account', 'category', 'transaction', 'installment_plan', 'budget')),
    entity_id  UUID NOT NULL,
    version    BIGINT NOT NULL,
    op         TEXT NOT NULL CHECK (op IN ('create', 'update', 'delete')),
    at         TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX change_log_user_seq_idx ON change_log (user_id, seq);
