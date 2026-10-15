-- Versioned process graphs. A key/version row is written once by the first
-- authorized start and never changed; runs and their work carry the same
-- fingerprint so a changed contribution under an existing version is refused.
CREATE TABLE base_workflow_process_versions (
 id bigserial PRIMARY KEY, definition_key text NOT NULL, definition_version integer NOT NULL CHECK(definition_version>=1),
 definition_fingerprint text NOT NULL CHECK(length(definition_fingerprint)=64),
 created_at timestamptz NOT NULL DEFAULT now(), UNIQUE(definition_key,definition_version)
);
CREATE FUNCTION base_workflow_process_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'workflow process versions and events are append-only'; END $$;
CREATE TRIGGER base_workflow_process_versions_immutable BEFORE UPDATE OR DELETE ON base_workflow_process_versions
FOR EACH ROW EXECUTE FUNCTION base_workflow_process_immutable();
CREATE TABLE base_workflow_process_runs (
 id bigserial PRIMARY KEY, tenant_id bigint NOT NULL REFERENCES tenants(id),
 definition_key text NOT NULL, definition_version integer NOT NULL,
 definition_fingerprint text NOT NULL CHECK(length(definition_fingerprint)=64),
 status text NOT NULL CHECK(status IN ('running','paused','completed','failed','blocked')),
 priority integer NOT NULL DEFAULT 0, subject_type text NOT NULL, subject_id bigint NOT NULL CHECK(subject_id>0),
 owner text NOT NULL, correlation_key text, input jsonb NOT NULL DEFAULT '{}', output jsonb,
 idempotency_key text NOT NULL UNIQUE, last_error text,
 started_at timestamptz NOT NULL DEFAULT now(), available_at timestamptz NOT NULL DEFAULT now(),
 heartbeat_at timestamptz, paused_at timestamptz, pause_reason text, completed_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(definition_key,definition_version) REFERENCES base_workflow_process_versions(definition_key,definition_version)
);
CREATE INDEX base_workflow_process_runs_subject ON base_workflow_process_runs(tenant_id,subject_type,subject_id);
CREATE INDEX base_workflow_process_runs_definition ON base_workflow_process_runs(definition_key,definition_version);
CREATE INDEX base_workflow_process_runs_status ON base_workflow_process_runs(status,available_at,priority);
CREATE INDEX base_workflow_process_runs_correlation ON base_workflow_process_runs(tenant_id,correlation_key);
CREATE TABLE base_workflow_process_work_items (
 id bigserial PRIMARY KEY, tenant_id bigint NOT NULL REFERENCES tenants(id),
 run_id bigint NOT NULL REFERENCES base_workflow_process_runs(id),
 step_key text NOT NULL, label text NOT NULL, executor_key text NOT NULL,
 status text NOT NULL CHECK(status IN ('pending','available','leased','completed','failed','waived','blocked')),
 dependency_mode text NOT NULL DEFAULT 'all' CHECK(dependency_mode IN ('all','any')),
 required_signal text, signalled_at timestamptz, signal_payload jsonb,
 delay_seconds integer NOT NULL DEFAULT 0 CHECK(delay_seconds>=0), available_at timestamptz,
 attempts integer NOT NULL DEFAULT 0 CHECK(attempts>=0), max_attempts integer NOT NULL DEFAULT 1 CHECK(max_attempts>=1),
 priority integer NOT NULL DEFAULT 0, lease_owner text, lease_token text UNIQUE, lease_expires_at timestamptz,
 heartbeat_at timestamptz, outcome text, input jsonb NOT NULL DEFAULT '{}', input_ref text, output jsonb,
 result_ref text, metadata jsonb NOT NULL DEFAULT '{}', last_error text, completed_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 version bigint NOT NULL DEFAULT 1 CHECK(version>=1), UNIQUE(run_id,step_key)
);
CREATE INDEX base_workflow_process_work_available ON base_workflow_process_work_items(status,priority,available_at);
CREATE INDEX base_workflow_process_work_lease ON base_workflow_process_work_items(lease_expires_at);
CREATE INDEX base_workflow_process_work_tenant ON base_workflow_process_work_items(tenant_id,status);
CREATE TABLE base_workflow_process_dependencies (
 id bigserial PRIMARY KEY, tenant_id bigint NOT NULL REFERENCES tenants(id),
 work_item_id bigint NOT NULL REFERENCES base_workflow_process_work_items(id),
 depends_on_work_item_id bigint NOT NULL REFERENCES base_workflow_process_work_items(id),
 acceptable_outcomes jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(work_item_id,depends_on_work_item_id), CHECK(work_item_id<>depends_on_work_item_id)
);
CREATE INDEX base_workflow_process_dependencies_prerequisite ON base_workflow_process_dependencies(depends_on_work_item_id);
CREATE TABLE base_workflow_process_events (
 id bigserial PRIMARY KEY, tenant_id bigint NOT NULL REFERENCES tenants(id),
 run_id bigint NOT NULL REFERENCES base_workflow_process_runs(id),
 work_item_id bigint REFERENCES base_workflow_process_work_items(id),
 sequence bigint NOT NULL CHECK(sequence>=1), type text NOT NULL, payload jsonb,
 idempotency_key text UNIQUE, occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 UNIQUE(run_id,sequence)
);
CREATE INDEX base_workflow_process_events_tenant ON base_workflow_process_events(tenant_id,occurred_at);
CREATE TRIGGER base_workflow_process_events_immutable BEFORE UPDATE OR DELETE ON base_workflow_process_events
FOR EACH ROW EXECUTE FUNCTION base_workflow_process_immutable();
