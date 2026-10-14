-- Queue owns all transport state. No Core FK: historical actor references survive
-- account deletion; the executor must reprove the actor before each attempt.
CREATE TABLE base_queue_jobs (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    tenant_id bigint NOT NULL CHECK (tenant_id > 0),
    worker_id varchar(128) NOT NULL,
    worker_module text NOT NULL,
    queue text NOT NULL DEFAULT 'default' CHECK (queue = 'default'),
    args jsonb NOT NULL CHECK (jsonb_typeof(args) = 'object' AND octet_length(args::text) <= 131072),
    actor_kind text NOT NULL CHECK (actor_kind = 'user'),
    actor_id bigint NOT NULL CHECK (actor_id > 0),
    person_id bigint CHECK (person_id > 0),
    organisation_id bigint CHECK (organisation_id > 0),
    affiliation_id bigint CHECK (affiliation_id > 0),
    trace_id text NOT NULL,
    state text NOT NULL DEFAULT 'available' CHECK (state IN ('available','scheduled','executing','retryable','completed','cancelled','discarded')),
    attempt integer NOT NULL DEFAULT 0 CHECK (attempt >= 0 AND attempt <= max_attempts),
    max_attempts integer NOT NULL CHECK (max_attempts BETWEEN 1 AND 100),
    available_at timestamptz NOT NULL DEFAULT now(),
    inserted_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    attempted_at timestamptz,
    completed_at timestamptz,
    cancelled_at timestamptz,
    discarded_at timestamptz,
    lease_token text,
    lease_expires_at timestamptz,
    failure_code varchar(64),
    CHECK (affiliation_id IS NULL OR (person_id IS NOT NULL AND organisation_id IS NOT NULL)),
    CHECK ((state = 'executing') = (lease_token IS NOT NULL AND lease_expires_at IS NOT NULL)),
    CHECK ((lease_token IS NULL) = (lease_expires_at IS NULL)),
    CHECK (failure_code IS NULL OR failure_code ~ '^[a-z][a-z0-9_]{0,63}$')
);
CREATE INDEX base_queue_claim ON base_queue_jobs (available_at,id) WHERE state IN ('available','scheduled','retryable');
CREATE INDEX base_queue_expired_leases ON base_queue_jobs (lease_expires_at,id) WHERE state = 'executing';
CREATE INDEX base_queue_tenant_jobs ON base_queue_jobs (tenant_id,id DESC);
