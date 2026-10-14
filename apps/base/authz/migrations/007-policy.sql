-- Additive ownership transition: do not replay the original demo schema.
ALTER TABLE roles ADD COLUMN code text;
ALTER TABLE roles ADD COLUMN is_system boolean NOT NULL DEFAULT false;
ALTER TABLE roles ADD COLUMN description text NOT NULL DEFAULT '';
CREATE UNIQUE INDEX roles_system_code ON roles(tenant_id,code) WHERE code IS NOT NULL;
ALTER TABLE field_restrictions DROP CONSTRAINT field_restrictions_check;
ALTER TABLE field_restrictions ADD CHECK(length(table_id) BETWEEN 1 AND 200 AND length(field_id) BETWEEN 1 AND 200);
ALTER TABLE authz_decision_log ADD COLUMN reason text NOT NULL DEFAULT 'legacy';
ALTER TABLE authz_decision_log ADD COLUMN principal_type text NOT NULL DEFAULT 'user';
ALTER TABLE authz_decision_log ADD COLUMN system_principal text;
CREATE TABLE authz_system_grants (
 tenant_id bigint NOT NULL REFERENCES tenants(id),
 principal text NOT NULL CHECK(length(principal) BETWEEN 1 AND 100),
 capability text NOT NULL CHECK(length(capability) BETWEEN 1 AND 200),
 is_allowed boolean NOT NULL,
 PRIMARY KEY(tenant_id,principal,capability)
);
