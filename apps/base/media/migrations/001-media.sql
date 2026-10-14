-- Asset metadata only: bytes live on operator-mapped private disks. `private`
-- marks the bytes of a private attachment (M2); generic stream, derivative and
-- delete paths refuse any tree that contains one, in either direction.
CREATE TABLE base_media_assets (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 tenant_id bigint NOT NULL REFERENCES tenants(id),
 parent_id bigint,
 disk text NOT NULL CHECK (length(disk) BETWEEN 1 AND 255),
 storage_key text NOT NULL CHECK (length(storage_key) BETWEEN 1 AND 255),
 original_filename text CHECK (length(original_filename) <= 255),
 mime_type text CHECK (length(mime_type) <= 255),
 file_size bigint CHECK (file_size >= 0),
 kind text NOT NULL CHECK (length(kind) BETWEEN 1 AND 255),
 private boolean NOT NULL DEFAULT false,
 metadata jsonb,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE (tenant_id, id),
 CHECK ((kind = 'original') = (parent_id IS NULL)),
 -- A derivative always belongs to its parent's tenant. NO ACTION lets one
 -- statement remove a whole tree but never a parent alone.
 FOREIGN KEY (tenant_id, parent_id) REFERENCES base_media_assets (tenant_id, id)
);
-- Two rows must never share bytes: deleting one would delete the other's file.
CREATE UNIQUE INDEX base_media_assets_location_idx ON base_media_assets (disk, storage_key) WHERE disk <> 'external';
CREATE INDEX base_media_assets_parent_idx ON base_media_assets (parent_id, kind);
