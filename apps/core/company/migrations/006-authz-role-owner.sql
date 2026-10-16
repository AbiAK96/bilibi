-- Core Company contributes the scoped foreign key behind Authz's bare
-- `roles.company_id` (K15, Bilimbi `base_authz_roles_company_foreign`): an
-- owning company is a company of the role's own tenant, and a company that
-- owns roles cannot be deleted from under them.
ALTER TABLE roles ADD CONSTRAINT roles_company_foreign
  FOREIGN KEY (tenant_id, company_id) REFERENCES companies(tenant_id, id) ON DELETE RESTRICT;
