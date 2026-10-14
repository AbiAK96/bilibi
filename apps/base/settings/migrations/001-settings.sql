CREATE TABLE base_settings (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key varchar(255) NOT NULL CHECK (length(key) > 0),
    tenant_id bigint NOT NULL,
    scope_type text NOT NULL CHECK (scope_type IN ('global','tenant','company','user')),
    scope_id bigint NOT NULL,
    value jsonb NOT NULL,
    is_encrypted boolean NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK ((scope_type = 'global' AND tenant_id = 0 AND scope_id = 0)
        OR (scope_type <> 'global' AND tenant_id > 0 AND scope_id > 0)),
    CHECK (scope_type <> 'tenant' OR scope_id = tenant_id),
    UNIQUE (key, tenant_id, scope_type, scope_id)
);
