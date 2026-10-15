-- Records-grade retention (A2). A retained document carries `retain_until`
-- instead of `expires_at` and may sit under a legal hold. Every retention change
-- is one append-only event row whose time PostgreSQL stamps. The facade refuses
-- first; these constraints and triggers refuse whatever bypasses it.
ALTER TABLE base_artifacts
    ALTER COLUMN expires_at DROP NOT NULL,
    ADD COLUMN retain_until timestamptz,
    ADD COLUMN legal_hold_at timestamptz,
    ADD COLUMN purge_attempts integer NOT NULL DEFAULT 0 CHECK (purge_attempts >= 0),
    ADD COLUMN purge_last_error text CHECK (length(purge_last_error) <= 255),
    ADD COLUMN purge_attempted_at timestamptz,
    ADD COLUMN purge_held_at timestamptz;
ALTER TABLE base_artifacts
    ADD CONSTRAINT base_artifacts_retention_exclusive CHECK ((retain_until IS NULL) <> (expires_at IS NULL)),
    ADD CONSTRAINT base_artifacts_hold_requires_retention CHECK (legal_hold_at IS NULL OR retain_until IS NOT NULL);
CREATE INDEX base_artifacts_purge_hold ON base_artifacts(tenant_id,company_id,owner_id,purge_held_at,id)
    WHERE purge_held_at IS NOT NULL AND purged_at IS NULL;

CREATE TABLE base_artifact_retention_events (
    id bigserial PRIMARY KEY,
    artifact_id bigint NOT NULL REFERENCES base_artifacts(id) ON DELETE RESTRICT,
    tenant_id bigint NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    company_id bigint NOT NULL CHECK (company_id > 0),
    owner_id text NOT NULL CHECK (length(owner_id) BETWEEN 1 AND 255),
    event text NOT NULL CHECK (event IN ('retained','retention_extended','hold_placed','hold_released')),
    retain_until timestamptz NOT NULL,
    legal_hold boolean NOT NULL,
    reason varchar(255),
    actor_type varchar(40) NOT NULL CHECK (actor_type IN ('user','agent','guest','console','scheduler','queue','system','unknown')),
    actor_id bigint NOT NULL,
    impersonator_id bigint,
    system_principal varchar(100),
    trace_id text,
    -- Stamped by the trigger below; the application cannot supply it.
    recorded_at timestamptz NOT NULL,
    CHECK (system_principal IS NULL OR actor_type = 'system'),
    CHECK (actor_type <> 'system' OR actor_id = 0)
);
CREATE INDEX base_artifact_retention_events_artifact ON base_artifact_retention_events(artifact_id,id);

CREATE FUNCTION base_artifacts_retention_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.retain_until IS NOT NULL
           AND (OLD.legal_hold_at IS NOT NULL OR OLD.retain_until > clock_timestamp()) THEN
            RAISE EXCEPTION 'artifact % is retained', OLD.public_id
                USING ERRCODE = 'check_violation', CONSTRAINT = 'base_artifacts_retention_guard';
        END IF;
        RETURN OLD;
    END IF;
    IF OLD.retain_until IS NOT NULL
       AND (NEW.retain_until IS NULL OR NEW.retain_until < OLD.retain_until) THEN
        RAISE EXCEPTION 'artifact % retention cannot be shortened', OLD.public_id
            USING ERRCODE = 'check_violation', CONSTRAINT = 'base_artifacts_retention_guard';
    END IF;
    IF OLD.retain_until IS NOT NULL
       AND (OLD.legal_hold_at IS NOT NULL OR OLD.retain_until > clock_timestamp())
       AND ((NEW.deleted_at IS NOT NULL AND OLD.deleted_at IS NULL)
            OR (NEW.purged_at IS NOT NULL AND OLD.purged_at IS NULL)) THEN
        RAISE EXCEPTION 'artifact % is retained', OLD.public_id
            USING ERRCODE = 'check_violation', CONSTRAINT = 'base_artifacts_retention_guard';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER base_artifacts_retention_guard BEFORE UPDATE OR DELETE ON base_artifacts
    FOR EACH ROW EXECUTE FUNCTION base_artifacts_retention_guard();

CREATE FUNCTION base_artifact_retention_events_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        NEW.recorded_at := clock_timestamp();
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'base_artifact_retention_events is append-only'
        USING ERRCODE = 'check_violation', CONSTRAINT = 'base_artifact_retention_events_append_only';
END $$;
CREATE TRIGGER base_artifact_retention_events_append_only BEFORE INSERT OR UPDATE OR DELETE
    ON base_artifact_retention_events FOR EACH ROW EXECUTE FUNCTION base_artifact_retention_events_guard();
CREATE TRIGGER base_artifact_retention_events_no_truncate BEFORE TRUNCATE
    ON base_artifact_retention_events FOR EACH STATEMENT EXECUTE FUNCTION base_artifact_retention_events_guard();
