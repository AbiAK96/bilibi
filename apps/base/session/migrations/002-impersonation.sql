-- The operator behind an impersonated session. Bilimbi keeps it in the signed
-- cookie; Bilibi's cookie is an opaque token, so the durable row is the only
-- place it lives and a browser can never name it. `user_id` stays the account
-- acted as. Core User adds the scoped account foreign key.
ALTER TABLE sessions ADD COLUMN impersonator_id bigint,
 ADD CONSTRAINT sessions_impersonator_not_actor CHECK (impersonator_id IS NULL OR impersonator_id <> user_id);
CREATE INDEX sessions_impersonator_idx ON sessions(tenant_id, impersonator_id) WHERE impersonator_id IS NOT NULL;
