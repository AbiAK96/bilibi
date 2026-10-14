-- Private attachments: a subject owner's draft or submitted file. The bytes are
-- a `private` asset; only this table binds them to a subject, so the generic
-- stream, derivative and delete paths can never reach them.
CREATE TABLE base_media_attachments (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 public_id text NOT NULL UNIQUE CHECK (public_id ~ '^[0-9A-HJKMNP-TV-Z]{26}$'),
 tenant_id bigint NOT NULL REFERENCES tenants(id),
 media_asset_id bigint NOT NULL,
 subject_type text NOT NULL CHECK (length(subject_type) BETWEEN 1 AND 255),
 subject_id text NOT NULL CHECK (length(subject_id) BETWEEN 1 AND 255),
 state text NOT NULL CHECK (state IN ('draft', 'submitted')),
 uploaded_by_type text NOT NULL CHECK (uploaded_by_type = 'user'),
 uploaded_by_id bigint NOT NULL,
 submitted_at timestamptz,
 retained_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE (media_asset_id),
 CHECK ((state = 'submitted') = (submitted_at IS NOT NULL)),
 -- The file stays in the uploader's tenant, and an asset row cannot be deleted
 -- while a reference exists (NO ACTION), so evidence cannot lose its bytes.
 FOREIGN KEY (tenant_id, media_asset_id) REFERENCES base_media_assets (tenant_id, id)
);
CREATE INDEX base_media_attachments_subject_idx ON base_media_attachments (tenant_id, subject_type, subject_id);
CREATE INDEX base_media_attachments_cleanup_idx ON base_media_attachments (tenant_id, created_at) WHERE state = 'draft' AND retained_at IS NULL;

-- Defence in depth for the application rules. Submitted evidence is immutable,
-- a binding never moves, and a draft only advances to submitted or gains
-- `retained_at`, so a stray UPDATE or DELETE cannot rewrite what was submitted.
CREATE FUNCTION base_media_attachments_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP = 'DELETE' THEN
  IF OLD.state = 'submitted' THEN
   RAISE EXCEPTION 'submitted attachments are immutable' USING ERRCODE = 'restrict_violation';
  END IF;
  RETURN OLD;
 END IF;
 IF OLD.state = 'submitted'
  OR NEW.public_id IS DISTINCT FROM OLD.public_id
  OR NEW.tenant_id IS DISTINCT FROM OLD.tenant_id
  OR NEW.media_asset_id IS DISTINCT FROM OLD.media_asset_id
  OR NEW.subject_type IS DISTINCT FROM OLD.subject_type
  OR NEW.subject_id IS DISTINCT FROM OLD.subject_id
  OR NEW.uploaded_by_type IS DISTINCT FROM OLD.uploaded_by_type
  OR NEW.uploaded_by_id IS DISTINCT FROM OLD.uploaded_by_id
  OR NEW.created_at IS DISTINCT FROM OLD.created_at
  OR (OLD.retained_at IS NOT NULL AND NEW.retained_at IS DISTINCT FROM OLD.retained_at) THEN
  RAISE EXCEPTION 'attachment is immutable' USING ERRCODE = 'restrict_violation';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER base_media_attachments_guard BEFORE UPDATE OR DELETE ON base_media_attachments FOR EACH ROW EXECUTE FUNCTION base_media_attachments_guard();

-- Private bytes are always a lone original: no derivative may hang from a
-- private asset, and a private asset is never itself a derivative.
ALTER TABLE base_media_assets ADD CONSTRAINT base_media_assets_private_original CHECK (NOT private OR parent_id IS NULL);
CREATE FUNCTION base_media_assets_private_parent() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.parent_id IS NOT NULL AND EXISTS (SELECT 1 FROM base_media_assets WHERE id = NEW.parent_id AND private) THEN
  RAISE EXCEPTION 'private assets cannot have derivatives' USING ERRCODE = 'restrict_violation';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER base_media_assets_private_parent BEFORE INSERT OR UPDATE OF parent_id ON base_media_assets FOR EACH ROW EXECUTE FUNCTION base_media_assets_private_parent();
