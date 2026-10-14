-- Additive bridge only. Never edit/re-register the applied 001-base demo DDL.
-- Audit owns subsequent audit schema migrations; Database owns this boundary ledger.
ALTER TABLE audit_mutations ALTER COLUMN actor_id DROP NOT NULL;
ALTER TABLE audit_mutations ADD COLUMN context jsonb;
CREATE TABLE bilibi_action_commits (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 tenant_id bigint NOT NULL REFERENCES tenants(id),
 action text NOT NULL, trace_id text NOT NULL, context jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE bilibi_production_seeds (
 version text PRIMARY KEY, module_id text NOT NULL,
 contribution_id text NOT NULL, definition jsonb NOT NULL,
 applied_at timestamptz NOT NULL DEFAULT now()
);
