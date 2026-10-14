-- No existing tenant is silently promoted, including demo tenant 1.
-- Operator setup belongs to K17; callers still need Authz capability checks.
CREATE TABLE platform_operator (
 singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
 tenant_id bigint NOT NULL UNIQUE REFERENCES tenants(id)
);
