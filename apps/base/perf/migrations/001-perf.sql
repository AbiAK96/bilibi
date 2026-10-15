-- Bounded, redacted performance history. Identities are stable route templates,
-- worker ids and one process row; telemetry metadata, SQL, arguments, identifiers
-- and error payloads never reach this table, and the constraints keep it so.
CREATE TABLE base_perf_samples (
    id bigserial PRIMARY KEY,
    kind varchar(16) NOT NULL,
    identity varchar(255) NOT NULL,
    outcome varchar(16) NOT NULL,
    duration_ms bigint NOT NULL,
    db_duration_ms bigint NOT NULL DEFAULT 0,
    db_count integer NOT NULL DEFAULT 0,
    response_size_class varchar(16),
    -- Process facts, present on kind = 'runtime' rows only. NULL means the
    -- platform cannot report that fact, never zero.
    memory_bytes bigint,
    run_queue bigint,
    alive_tasks bigint,
    threads bigint,
    open_fds bigint,
    cpu_ms bigint,
    pool_in_use bigint,
    pool_waiting bigint,
    observed_at timestamptz NOT NULL,
    CONSTRAINT base_perf_samples_kind_check CHECK (kind IN ('request', 'event', 'job', 'runtime')),
    CONSTRAINT base_perf_samples_outcome_check CHECK (outcome IN ('ok', 'error', 'cancelled', 'discarded')),
    CONSTRAINT base_perf_samples_size_check CHECK (response_size_class IS NULL OR response_size_class IN ('under_1k', '1k_10k', '10k_100k', 'over_100k')),
    CONSTRAINT base_perf_samples_identity_check CHECK (
        (kind = 'request' AND identity ~ '^/[A-Za-z0-9_/{}:.*-]{0,254}$')
        OR (kind = 'event' AND identity ~ '^event:/[A-Za-z0-9_/{}:.*-]{0,248}$')
        OR (kind = 'job' AND identity ~ '^[a-z0-9][a-z0-9_/-]{0,127}$')
        OR (kind = 'runtime' AND identity = 'process')
    ),
    CONSTRAINT base_perf_samples_nonnegative_check CHECK (
        duration_ms >= 0 AND db_duration_ms >= 0 AND db_count >= 0
        AND (memory_bytes IS NULL OR memory_bytes >= 0)
        AND (run_queue IS NULL OR run_queue >= 0)
        AND (alive_tasks IS NULL OR alive_tasks >= 0)
        AND (threads IS NULL OR threads >= 0)
        AND (open_fds IS NULL OR open_fds >= 0)
        AND (cpu_ms IS NULL OR cpu_ms >= 0)
        AND (pool_in_use IS NULL OR pool_in_use >= 0)
        AND (pool_waiting IS NULL OR pool_waiting >= 0)
    )
);
CREATE INDEX base_perf_samples_observed_index ON base_perf_samples (observed_at, id);
CREATE INDEX base_perf_samples_identity_window_index ON base_perf_samples (kind, identity, observed_at, id);
