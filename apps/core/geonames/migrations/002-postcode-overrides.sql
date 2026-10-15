-- Operator corrections to postcodes (Bilimbi's `geonames_postcode_overrides`
-- sidecar). `geonames_postcodes` stays the Belimbing-compatible source and
-- lookup table with no provenance column. This table holds the operator's
-- desired record, a snapshot of the upstream row it supersedes (every
-- `source_*` column NULL for an operator-created row) and an optimistic
-- revision. The desired record is materialized into `geonames_postcodes`,
-- so every lookup keeps working, and materialized again after each refresh
-- of its country's postcodes.
--
-- `applied_postcode_id` is deliberately not a foreign key: a refresh deletes
-- the country's upstream rows before it re-materializes the corrections, and
-- the override must outlive the row it was applied to.
CREATE TABLE geonames_postcode_overrides (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 applied_postcode_id bigint NOT NULL,
 country_iso text NOT NULL REFERENCES geonames_countries (iso) ON UPDATE CASCADE ON DELETE RESTRICT,
 postcode text NOT NULL CHECK (length(postcode) BETWEEN 1 AND 20),
 place_name text NOT NULL CHECK (length(place_name) BETWEEN 1 AND 180),
 admin1_code text CHECK (length(admin1_code) <= 20),
 admin_name1 text CHECK (length(admin_name1) <= 100),
 admin_code1 text CHECK (length(admin_code1) <= 20),
 admin_name2 text CHECK (length(admin_name2) <= 100),
 admin_code2 text CHECK (length(admin_code2) <= 20),
 admin_name3 text CHECK (length(admin_name3) <= 100),
 admin_code3 text CHECK (length(admin_code3) <= 20),
 latitude numeric(10, 7) CHECK (latitude BETWEEN -90 AND 90),
 longitude numeric(10, 7) CHECK (longitude BETWEEN -180 AND 180),
 accuracy smallint CHECK (accuracy BETWEEN 1 AND 6),
 -- The upstream row at the moment of the first correction; matched exactly
 -- (NULL-safe) when the country is refreshed.
 source_country_iso text,
 source_postcode text,
 source_place_name text,
 source_admin1_code text,
 source_admin_name1 text,
 source_admin_code1 text,
 source_admin_name2 text,
 source_admin_code2 text,
 source_admin_name3 text,
 source_admin_code3 text,
 source_latitude numeric(10, 7),
 source_longitude numeric(10, 7),
 source_accuracy smallint,
 lock_version integer NOT NULL DEFAULT 1,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CONSTRAINT geonames_postcode_overrides_applied_postcode_id_unique UNIQUE (applied_postcode_id)
);
CREATE INDEX geonames_postcode_overrides_country_iso_postcode_index ON geonames_postcode_overrides (country_iso, postcode);
CREATE INDEX geonames_postcode_overrides_source_country_iso_index ON geonames_postcode_overrides (source_country_iso);
