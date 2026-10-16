-- Company-owned custom roles (K15, Bilimbi `base_authz_roles.company_id`).
-- Base keeps the column bare and nullable: the scoped foreign key to
-- `companies` is Core Company's to contribute, and custom roles created
-- before this migration stay unowned (labelled "Unowned", as Bilimbi labels
-- them) rather than being assigned an owner the operator never chose.
ALTER TABLE roles ADD COLUMN company_id bigint;
CREATE INDEX roles_company_id_idx ON roles(tenant_id,company_id);
