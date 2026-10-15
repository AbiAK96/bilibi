CREATE TABLE base_workflow_flows (
 id bigserial UNIQUE NOT NULL, code text PRIMARY KEY, owner text NOT NULL, subject_type text NOT NULL UNIQUE,
 label text NOT NULL, is_active boolean NOT NULL DEFAULT true
);
CREATE TABLE base_workflow_status_configs (
 id bigserial PRIMARY KEY, flow text NOT NULL REFERENCES base_workflow_flows(code),
 code text NOT NULL, label text NOT NULL, position integer NOT NULL DEFAULT 0,
 is_active boolean NOT NULL DEFAULT true, settings jsonb NOT NULL DEFAULT '{}',
 UNIQUE(flow,code)
);
CREATE TABLE base_workflow_edges (
 id bigserial PRIMARY KEY, flow text NOT NULL REFERENCES base_workflow_flows(code),
 from_code text NOT NULL, to_code text NOT NULL, label text NOT NULL,
 capability text, guard_key text, action_key text, position integer NOT NULL DEFAULT 0,
 is_active boolean NOT NULL DEFAULT true, sla_seconds bigint CHECK(sla_seconds>=0),
 metadata jsonb NOT NULL DEFAULT '{}', UNIQUE(flow,from_code,to_code),
 FOREIGN KEY(flow,from_code) REFERENCES base_workflow_status_configs(flow,code),
 FOREIGN KEY(flow,to_code) REFERENCES base_workflow_status_configs(flow,code)
);
CREATE TABLE base_workflow_bindings (
 id bigserial PRIMARY KEY, tenant_id bigint NOT NULL REFERENCES tenants(id),
 flow text NOT NULL REFERENCES base_workflow_flows(code), subject_type text NOT NULL,
 subject_id bigint NOT NULL CHECK(subject_id>0), owner text NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(), UNIQUE(flow,subject_id),
 UNIQUE(tenant_id,subject_type,subject_id)
);
CREATE TABLE base_workflow_status_history (
 id bigserial PRIMARY KEY, binding_id bigint NOT NULL REFERENCES base_workflow_bindings(id),
 from_status text NOT NULL, status text NOT NULL,
 kind text NOT NULL CHECK(kind IN ('transition','record_initial','record_comment')),
 subject_version bigint NOT NULL CHECK(subject_version>=0),
 actor_type text NOT NULL, actor_id text NOT NULL, impersonator jsonb,
 comment text, comment_tag text, assignees jsonb NOT NULL DEFAULT '[]',
 attachments jsonb NOT NULL DEFAULT '[]', metadata jsonb NOT NULL DEFAULT '{}',
 tat bigint CHECK(tat>=0), transitioned_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX base_workflow_history_subject ON base_workflow_status_history(binding_id,id);
-- History facts cannot be rewritten, including by an owning facade.
CREATE FUNCTION base_workflow_history_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'workflow history is append-only'; END $$;
CREATE TRIGGER base_workflow_history_immutable BEFORE UPDATE OR DELETE ON base_workflow_status_history
FOR EACH ROW EXECUTE FUNCTION base_workflow_history_immutable();
