-- Retention prunes by age across every tenant (authz.decision_log_retention_days),
-- which the tenant-leading indexes cannot serve.
CREATE INDEX authz_decision_log_created_idx ON authz_decision_log(created_at);
