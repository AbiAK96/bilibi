-- The one-time platform administrator bootstrap receipt (Bilimbi
-- `bilimbi_user_bootstrap`). One row ever: once it exists the bootstrap has
-- run, so a later seed never re-grants the administrator a revoked role.
CREATE TABLE user_bootstrap (
 id bigint PRIMARY KEY CHECK (id = 1),
 tenant_name text NOT NULL,
 company_name text NOT NULL,
 company_code text NOT NULL,
 admin_email text NOT NULL,
 tenant_id bigint NOT NULL REFERENCES tenants(id),
 user_id bigint NOT NULL,
 company_id bigint NOT NULL,
 completed_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(tenant_id,user_id) REFERENCES users(tenant_id,id)
);
