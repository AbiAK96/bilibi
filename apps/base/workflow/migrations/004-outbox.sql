-- One event per committed transition; delivery authority comes from the binding.
CREATE TABLE base_workflow_transition_outbox (
 id bigserial PRIMARY KEY,
 history_id bigint NOT NULL UNIQUE REFERENCES base_workflow_status_history(id),
 event_key text NOT NULL UNIQUE,
 transition_snapshot jsonb NOT NULL,
 attempts integer NOT NULL DEFAULT 0 CHECK(attempts>=0),
 available_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 lease_token text, lease_expires_at timestamptz,
 delivered_at timestamptz, last_error varchar(64),
 created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 CHECK ((lease_token IS NULL) = (lease_expires_at IS NULL))
);
CREATE INDEX base_workflow_outbox_due ON base_workflow_transition_outbox(available_at,id) WHERE delivered_at IS NULL;
-- Receipt and listener database effects commit together. A later failing
-- listener cannot make an earlier successful listener repeat its effect.
CREATE TABLE base_workflow_listener_receipts (
 outbox_id bigint NOT NULL REFERENCES base_workflow_transition_outbox(id) ON DELETE CASCADE,
 listener_key text NOT NULL, delivered_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 PRIMARY KEY(outbox_id,listener_key)
);
