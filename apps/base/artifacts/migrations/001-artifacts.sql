CREATE TABLE base_artifacts (
    id bigserial PRIMARY KEY,
    public_id text NOT NULL UNIQUE CHECK (public_id ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'),
    tenant_id bigint NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    company_id bigint NOT NULL CHECK (company_id > 0),
    owner_id text NOT NULL CHECK (length(owner_id) BETWEEN 1 AND 255),
    owner_adapter text NOT NULL CHECK (length(owner_adapter) BETWEEN 1 AND 1024),
    subject text NOT NULL CHECK (length(subject) BETWEEN 1 AND 255),
    kind text NOT NULL CHECK (length(kind) BETWEEN 1 AND 100),
    content_type text NOT NULL CHECK (length(content_type) BETWEEN 1 AND 255),
    byte_size bigint NOT NULL CHECK (byte_size BETWEEN 1 AND 536870912),
    sha256 text NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    storage_root text NOT NULL CHECK (left(storage_root,1) = '/'),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    ready_at timestamptz,
    deleted_at timestamptz,
    purged_at timestamptz,
    CHECK (expires_at > created_at),
    CHECK (purged_at IS NULL OR deleted_at IS NOT NULL)
);
CREATE INDEX base_artifacts_recovery ON base_artifacts(tenant_id,company_id,owner_id,created_at,id) WHERE purged_at IS NULL;
-- Storage identity is immutable: changing it could orphan bytes or substitute
-- authority. Purged evidence remains in the ledger; no metadata hard deletes.
CREATE FUNCTION base_artifacts_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'artifact evidence cannot be deleted';
    END IF;
    IF ROW(NEW.public_id,NEW.tenant_id,NEW.company_id,NEW.owner_id,NEW.owner_adapter,
           NEW.subject,NEW.kind,NEW.content_type,NEW.byte_size,NEW.sha256,NEW.storage_root,NEW.created_at)
       IS DISTINCT FROM
       ROW(OLD.public_id,OLD.tenant_id,OLD.company_id,OLD.owner_id,OLD.owner_adapter,
           OLD.subject,OLD.kind,OLD.content_type,OLD.byte_size,OLD.sha256,OLD.storage_root,OLD.created_at) THEN
        RAISE EXCEPTION 'artifact identity is immutable';
    END IF;
    IF (OLD.ready_at IS NOT NULL AND NEW.ready_at IS DISTINCT FROM OLD.ready_at)
       OR (OLD.deleted_at IS NOT NULL AND NEW.deleted_at IS DISTINCT FROM OLD.deleted_at)
       OR (OLD.purged_at IS NOT NULL AND NEW.purged_at IS DISTINCT FROM OLD.purged_at)
       OR (OLD.deleted_at IS NOT NULL AND NEW.ready_at IS DISTINCT FROM OLD.ready_at) THEN
        RAISE EXCEPTION 'artifact lifecycle cannot be reversed';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER base_artifacts_lifecycle BEFORE UPDATE OR DELETE ON base_artifacts FOR EACH ROW EXECUTE FUNCTION base_artifacts_guard();
