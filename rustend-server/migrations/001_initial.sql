CREATE TABLE clients (
    id            UUID PRIMARY KEY,
    registered_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE revisions (
    id          UUID PRIMARY KEY,
    object_id   UUID        NOT NULL,
    object_type TEXT        NOT NULL,
    deleted     BOOLEAN     NOT NULL DEFAULT FALSE,
    data        JSONB,
    created_at  TIMESTAMPTZ NOT NULL,
    created_by  UUID        NOT NULL REFERENCES clients(id),
    CONSTRAINT active_has_data CHECK (
        (deleted = FALSE AND data IS NOT NULL) OR
        (deleted = TRUE  AND data IS NULL)
    )
);

CREATE INDEX revisions_object_id  ON revisions(object_id);
CREATE INDEX revisions_created_at ON revisions(created_at);
CREATE INDEX revisions_data       ON revisions USING GIN (data jsonb_path_ops);

CREATE TABLE revision_parents (
    revision_id UUID NOT NULL REFERENCES revisions(id),
    parent_id   UUID NOT NULL REFERENCES revisions(id),
    PRIMARY KEY (revision_id, parent_id)
);

CREATE TABLE transactions (
    id         BIGSERIAL PRIMARY KEY,
    client_id  UUID        NOT NULL REFERENCES clients(id),
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE transaction_revisions (
    transaction_id BIGINT NOT NULL REFERENCES transactions(id),
    revision_id    UUID   NOT NULL REFERENCES revisions(id),
    PRIMARY KEY (transaction_id, revision_id)
);

CREATE TABLE object_heads (
    object_id   UUID NOT NULL,
    revision_id UUID NOT NULL REFERENCES revisions(id),
    PRIMARY KEY (object_id, revision_id)
);

CREATE TABLE files (
    object_id UUID PRIMARY KEY,
    data      BYTEA NOT NULL
);
