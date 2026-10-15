-- Bilibi-only runtime state. An occurrence row is the durable claim that one
-- intended instant of one definition was handed to the Queue, so two app
-- processes can never enqueue the same occurrence: (key, intended_at, trigger)
-- is unique over every state, including finished ones. overlap_key is set only
-- for forbid-overlap definitions and cleared when the occurrence finishes, so
-- the partial unique index is the single active lease per key. tenant_id is
-- the Queue tenant of the job so reconciliation can ask Queue's scoped reads.
CREATE TABLE base_schedule_occurrences (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key varchar(255) NOT NULL CHECK (key ~ '^[a-z0-9][a-z0-9._/-]{0,254}$'),
    intended_at timestamptz NOT NULL,
    trigger text NOT NULL CHECK (trigger IN ('scheduled','manual')),
    overlap_key varchar(266),
    state text NOT NULL CHECK (state IN ('queued','running','succeeded','failed')),
    tenant_id bigint NOT NULL CHECK (tenant_id > 0),
    job_id bigint CHECK (job_id > 0),
    claimed_at timestamptz NOT NULL DEFAULT now(),
    started_at timestamptz,
    finished_at timestamptz,
    CHECK ((state IN ('succeeded','failed')) = (finished_at IS NOT NULL)),
    CHECK (finished_at IS NULL OR overlap_key IS NULL),
    CONSTRAINT base_schedule_occurrences_intended_unique UNIQUE (key, intended_at, trigger)
);
CREATE UNIQUE INDEX base_schedule_occurrences_active_overlap_unique
    ON base_schedule_occurrences (overlap_key) WHERE overlap_key IS NOT NULL AND finished_at IS NULL;
CREATE INDEX base_schedule_occurrences_unfinished_claimed
    ON base_schedule_occurrences (claimed_at, id) WHERE finished_at IS NULL AND job_id IS NOT NULL;
CREATE INDEX base_schedule_occurrences_latest_scheduled
    ON base_schedule_occurrences (key, intended_at DESC) WHERE trigger = 'scheduled';

-- Best-effort operational run history, Bilimbi's base_schedule_runs shape
-- without the Laravel columns. A row per attempt, plus a 'skipped' row for an
-- occurrence refused for overlap. Who asked for a manual run is denormalised at
-- dispatch; no foreign key, Base never reads Core.
CREATE TABLE base_schedule_runs (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key varchar(255) NOT NULL,
    name varchar(255) NOT NULL,
    expression varchar(64),
    trigger text NOT NULL CHECK (trigger IN ('scheduled','manual')),
    triggered_by_user_id bigint CHECK (triggered_by_user_id > 0),
    triggered_by_name varchar(255),
    status text NOT NULL CHECK (status IN ('running','succeeded','failed','skipped')),
    started_at timestamptz NOT NULL,
    finished_at timestamptz,
    runtime_ms integer CHECK (runtime_ms >= 0),
    output_excerpt varchar(255),
    occurrence_id bigint,
    job_id bigint,
    attempt integer CHECK (attempt > 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK ((status = 'running') = (finished_at IS NULL))
);
CREATE INDEX base_schedule_runs_key_started ON base_schedule_runs (key, started_at DESC);
CREATE INDEX base_schedule_runs_started ON base_schedule_runs (started_at DESC);
