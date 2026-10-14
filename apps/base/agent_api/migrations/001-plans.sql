-- No credential, operation input or review content is retained here.
CREATE TABLE base_agent_api_plans (
    code_hash bytea PRIMARY KEY CHECK (octet_length(code_hash)=32),
    tenant_id bigint NOT NULL REFERENCES tenants(id),
    binding_hash bytea NOT NULL CHECK (octet_length(binding_hash)=32),
    operations_hash bytea NOT NULL CHECK (octet_length(operations_hash)=32),
    preview_hash bytea NOT NULL CHECK (octet_length(preview_hash)=32),
    expires_at timestamptz NOT NULL,
    used_at timestamptz
);
CREATE INDEX base_agent_api_plans_expiry ON base_agent_api_plans(tenant_id, expires_at);
-- Non-business, tenant-scoped specimen for real transaction walkthroughs.
CREATE TABLE base_agent_api_reference (
    tenant_id bigint PRIMARY KEY REFERENCES tenants(id),
    value bigint NOT NULL CHECK (value BETWEEN 0 AND 100),
    version bigint NOT NULL CHECK (version>0)
);
