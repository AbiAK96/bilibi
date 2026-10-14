-- Audit evidence: Bilimbi's final base_audit shape on native timestamptz columns.
-- Rows outlive actors and subjects: no foreign keys to tenants, users or records.
CREATE TABLE base_audit_mutations (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 company_id bigint, tenant_id bigint,
 actor_type varchar(40) NOT NULL, actor_id bigint NOT NULL, actor_role varchar(100),
 impersonator_id bigint, system_principal varchar(100),
 ip_address inet, url text, user_agent varchar(80),
 auditable_type varchar(255) NOT NULL, auditable_id varchar(128) NOT NULL,
 subject_name varchar(255), subject_id varchar(128), subject_identifier varchar(255),
 source varchar(20) NOT NULL DEFAULT 'boundary', event varchar(20) NOT NULL,
 old_values jsonb, new_values jsonb, trace_id text,
 occurred_at timestamptz NOT NULL, recorded_at timestamptz(6),
 CHECK (actor_type IN ('user','agent','guest','console','scheduler','queue','system','unknown')),
 CHECK (system_principal IS NULL OR actor_type = 'system'),
 CHECK (actor_type <> 'system' OR actor_id = 0)
);
CREATE INDEX base_audit_mutations_tenant_timeline_index ON base_audit_mutations(tenant_id, occurred_at DESC, id DESC);
CREATE INDEX base_audit_mutations_auditable_occurred_index ON base_audit_mutations(auditable_type, auditable_id, occurred_at);
CREATE INDEX base_audit_mutations_actor_occurred_index ON base_audit_mutations(actor_type, actor_id, occurred_at);
CREATE INDEX base_audit_mutations_event_index ON base_audit_mutations(event);
CREATE INDEX base_audit_mutations_trace_index ON base_audit_mutations(trace_id);
CREATE INDEX base_audit_mutations_company_index ON base_audit_mutations(company_id);
CREATE INDEX base_audit_mutations_impersonator_index ON base_audit_mutations(impersonator_id) WHERE impersonator_id IS NOT NULL;
CREATE INDEX base_audit_mutations_system_principal_index ON base_audit_mutations(system_principal) WHERE system_principal IS NOT NULL;
CREATE INDEX base_audit_mutations_subject_idx ON base_audit_mutations(subject_name, subject_id, subject_identifier, occurred_at) WHERE subject_name IS NOT NULL;

CREATE TABLE base_audit_actions (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 company_id bigint, tenant_id bigint,
 actor_type varchar(40) NOT NULL, actor_id bigint NOT NULL, actor_role varchar(100),
 impersonator_id bigint, system_principal varchar(100),
 ip_address inet, url text, user_agent varchar(80),
 event varchar(255) NOT NULL, payload jsonb, trace_id text,
 is_retained boolean NOT NULL DEFAULT false,
 occurred_at timestamptz NOT NULL, recorded_at timestamptz(6),
 CHECK (actor_type IN ('user','agent','guest','console','scheduler','queue','system','unknown')),
 CHECK (system_principal IS NULL OR actor_type = 'system'),
 CHECK (actor_type <> 'system' OR actor_id = 0)
);
CREATE INDEX base_audit_actions_tenant_timeline_index ON base_audit_actions(tenant_id, occurred_at DESC, id DESC);
CREATE INDEX base_audit_actions_event_occurred_index ON base_audit_actions(event, occurred_at);
CREATE INDEX base_audit_actions_actor_occurred_index ON base_audit_actions(actor_type, actor_id, occurred_at);
CREATE INDEX base_audit_actions_trace_index ON base_audit_actions(trace_id);
CREATE INDEX base_audit_actions_company_index ON base_audit_actions(company_id);
CREATE INDEX base_audit_actions_impersonator_index ON base_audit_actions(impersonator_id) WHERE impersonator_id IS NOT NULL;
CREATE INDEX base_audit_actions_system_principal_index ON base_audit_actions(system_principal) WHERE system_principal IS NOT NULL;

-- The recorded is_retained bit is immutable evidence; bookmarks are mutable metadata.
CREATE TABLE base_audit_action_metadata (
 action_id bigint PRIMARY KEY REFERENCES base_audit_actions(id) ON DELETE RESTRICT,
 is_retained boolean NOT NULL DEFAULT false
);
-- Diagnostics for a refused capture, written outside the rolled-back business
-- transaction. Identity and error type only: never values or messages.
CREATE TABLE base_audit_capture_failures (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 stage text NOT NULL, auditable_type text NOT NULL, error_type text NOT NULL,
 actor_type varchar(40) NOT NULL, actor_id bigint NOT NULL,
 impersonator_id bigint, system_principal varchar(100),
 company_id bigint, tenant_id bigint, trace_id text,
 recorded_at timestamptz(6) NOT NULL
);

CREATE FUNCTION base_audit_evidence_v1() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  IF TG_OP <> 'INSERT' THEN
    RAISE EXCEPTION 'audit evidence is append-only' USING ERRCODE = '42501';
  END IF;
  NEW.recorded_at := clock_timestamp();
  -- Nested: PL/pgSQL may evaluate both operands, and diagnostics have no event time.
  IF TG_TABLE_NAME <> 'base_audit_capture_failures' THEN
    IF NEW.occurred_at IS NULL THEN
      NEW.occurred_at := date_trunc('second', NEW.recorded_at);
    END IF;
  END IF;
  RETURN NEW;
END;
$$;
CREATE TRIGGER base_audit_mutations_evidence_guard BEFORE INSERT OR UPDATE OR DELETE ON base_audit_mutations FOR EACH ROW EXECUTE FUNCTION base_audit_evidence_v1();
CREATE TRIGGER base_audit_mutations_truncate_guard BEFORE TRUNCATE ON base_audit_mutations FOR EACH STATEMENT EXECUTE FUNCTION base_audit_evidence_v1();
CREATE TRIGGER base_audit_actions_evidence_guard BEFORE INSERT OR UPDATE OR DELETE ON base_audit_actions FOR EACH ROW EXECUTE FUNCTION base_audit_evidence_v1();
CREATE TRIGGER base_audit_actions_truncate_guard BEFORE TRUNCATE ON base_audit_actions FOR EACH STATEMENT EXECUTE FUNCTION base_audit_evidence_v1();
CREATE TRIGGER base_audit_capture_failures_evidence_guard BEFORE INSERT OR UPDATE OR DELETE ON base_audit_capture_failures FOR EACH ROW EXECUTE FUNCTION base_audit_evidence_v1();
CREATE TRIGGER base_audit_capture_failures_truncate_guard BEFORE TRUNCATE ON base_audit_capture_failures FOR EACH STATEMENT EXECUTE FUNCTION base_audit_evidence_v1();

-- One-time carry-over of the frozen demo ledgers (001-base, 004-database-boundary).
-- Those tables stay untouched; nothing writes to them after this version.
INSERT INTO base_audit_mutations(tenant_id, actor_type, actor_id, impersonator_id, system_principal,
 auditable_type, auditable_id, source, event, old_values, new_values, trace_id, occurred_at)
SELECT m.tenant_id,
 CASE WHEN m.actor_id IS NOT NULL THEN 'user'
      WHEN m.context->'actor'->>'kind' = 'system' THEN 'system' ELSE 'unknown' END,
 COALESCE(m.actor_id, 0),
 CASE WHEN m.context->'impersonator'->>'kind' = 'user' AND m.context->'impersonator'->>'id' ~ '^[0-9]{1,18}$'
      THEN (m.context->'impersonator'->>'id')::bigint END,
 CASE WHEN m.actor_id IS NULL AND m.context->'actor'->>'kind' = 'system' AND m.context->'actor'->>'id' <> '0'
      THEN left(m.context->'actor'->>'id', 100) END,
 left(m.auditable_type, 255), m.auditable_id::text, 'legacy',
 CASE WHEN m.old_values = '{}'::jsonb THEN 'created' WHEN m.new_values = '{}'::jsonb THEN 'deleted' ELSE 'updated' END,
 m.old_values, m.new_values, m.trace_id, m.created_at
FROM audit_mutations m ORDER BY m.id;
INSERT INTO base_audit_actions(tenant_id, actor_type, actor_id, impersonator_id, system_principal, event, payload, trace_id, occurred_at)
SELECT c.tenant_id,
 CASE WHEN c.context->'actor'->>'kind' = 'user' AND c.context->'actor'->>'id' ~ '^[0-9]{1,18}$' THEN 'user'
      WHEN c.context->'actor'->>'kind' = 'system' THEN 'system' ELSE 'unknown' END,
 CASE WHEN c.context->'actor'->>'kind' = 'user' AND c.context->'actor'->>'id' ~ '^[0-9]{1,18}$'
      THEN (c.context->'actor'->>'id')::bigint ELSE 0 END,
 CASE WHEN c.context->'impersonator'->>'kind' = 'user' AND c.context->'impersonator'->>'id' ~ '^[0-9]{1,18}$'
      THEN (c.context->'impersonator'->>'id')::bigint END,
 CASE WHEN c.context->'actor'->>'kind' = 'system' AND c.context->'actor'->>'id' <> '0'
      THEN left(c.context->'actor'->>'id', 100) END,
 left(c.action, 255), jsonb_build_object('result', 'committed', 'source', 'legacy'), c.trace_id, c.created_at
FROM bilibi_action_commits c ORDER BY c.id;
