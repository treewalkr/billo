-- Mutations applied through `POST /api/v1/sync/push`, so a replayed mutation returns its
-- original result instead of being applied again. Written in the same transaction as
-- the change itself.
CREATE TABLE sync_mutations (
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    mutation_id  UUID NOT NULL,
    result       JSONB NOT NULL,
    at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, mutation_id)
);
