-- Installation-global operator state. Definitions are code (immutable
-- contributions); only the decision about them is stored. Keys are not foreign
-- keys: a removed definition leaves its row as orphan evidence for an operator
-- to dispose of, and a renamed key can never inherit a decision.
CREATE TABLE base_schedule_definition_reviews (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key varchar(255) NOT NULL UNIQUE CHECK (key ~ '^[a-z0-9][a-z0-9._/-]{0,254}$'),
    fingerprint char(64) NOT NULL CHECK (fingerprint ~ '^[0-9a-f]{64}$'),
    enabled boolean NOT NULL,
    reviewed_at timestamptz NOT NULL DEFAULT now()
);
-- A row means paused; deleting it means resumed.
CREATE TABLE base_schedule_suppressions (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key varchar(255) NOT NULL UNIQUE CHECK (key ~ '^[a-z0-9][a-z0-9._/-]{0,254}$'),
    suppressed_at timestamptz NOT NULL DEFAULT now()
);
