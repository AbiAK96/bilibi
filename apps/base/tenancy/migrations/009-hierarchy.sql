-- Bilimbi's tenants carry a parent and timestamps (compatibility baseline
-- 20260811093951). The platform-operator marker stays the explicit singleton in
-- platform_operator (005): a tenant row never says it is the operator itself.
ALTER TABLE tenants
 ADD COLUMN parent_id bigint REFERENCES tenants(id),
 ADD COLUMN created_at timestamptz,
 ADD COLUMN updated_at timestamptz;
CREATE INDEX tenants_parent_id_idx ON tenants(parent_id);
CREATE INDEX tenants_status_idx ON tenants(status);
CREATE INDEX tenants_parent_id_status_idx ON tenants(parent_id, status);
