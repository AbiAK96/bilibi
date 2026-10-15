-- Human action requests. One row per tenant and idempotency key, holding only
-- the intent digest and the saved result; a repeated request replays the
-- result and never runs the owner handler again. The request is written and
-- completed in the same transaction as the owner write and work completion,
-- so a committed row always carries its result.
CREATE TABLE base_workflow_human_action_requests (
 id bigserial PRIMARY KEY, tenant_id bigint NOT NULL REFERENCES tenants(id),
 idempotency_key text NOT NULL CHECK(length(idempotency_key) BETWEEN 1 AND 255),
 intent_hash text NOT NULL CHECK(length(intent_hash)=64),
 action_key text NOT NULL, subject_type text NOT NULL, subject_id bigint NOT NULL CHECK(subject_id>0),
 process_run_id bigint REFERENCES base_workflow_process_runs(id),
 work_item_id bigint REFERENCES base_workflow_process_work_items(id),
 actor_type text NOT NULL, actor_id bigint NOT NULL, result jsonb, completed_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(tenant_id,idempotency_key),
 CHECK((completed_at IS NULL)=(result IS NULL)),
 CHECK((process_run_id IS NULL)=(work_item_id IS NULL))
);
CREATE INDEX base_workflow_human_action_requests_subject ON base_workflow_human_action_requests(tenant_id,subject_type,subject_id);
