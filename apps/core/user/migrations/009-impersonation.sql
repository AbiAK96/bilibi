-- An impersonated session names its operator by an account of the same tenant.
ALTER TABLE sessions ADD CONSTRAINT sessions_scoped_impersonator_fk FOREIGN KEY(tenant_id,impersonator_id) REFERENCES users(tenant_id,id);
