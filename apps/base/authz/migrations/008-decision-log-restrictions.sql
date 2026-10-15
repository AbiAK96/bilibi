-- Decision-log read model (P8): the resource a decision concerned and the user an
-- actor acted for. Existing rows have neither, and the list shows an em dash.
ALTER TABLE authz_decision_log ADD COLUMN resource_type text CHECK(length(resource_type) BETWEEN 1 AND 255);
ALTER TABLE authz_decision_log ADD COLUMN resource_id text CHECK(length(resource_id) BETWEEN 1 AND 255);
ALTER TABLE authz_decision_log ADD COLUMN acting_for_user_id bigint;
-- Every sort is a column followed by id, so a page boundary is stable.
CREATE INDEX authz_decision_log_timeline_idx ON authz_decision_log(tenant_id,created_at DESC,id DESC);
-- When a role's restriction of a field began, for the restriction list.
ALTER TABLE field_restrictions ADD COLUMN created_at timestamptz NOT NULL DEFAULT now();
