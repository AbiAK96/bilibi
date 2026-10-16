-- K9: the account's optional party links and the global login identity.
-- An account may name a Person and, through one of that Person's affiliations,
-- an Organisation. The affiliation's endpoints are stored beside it and held
-- consistent by the scoped foreign key Affiliation published for this purpose,
-- so a link can never name a Person or company other than the affiliation's.
ALTER TABLE users
 ADD COLUMN person_id bigint,
 ADD COLUMN affiliation_id bigint,
 ADD COLUMN organisation_id bigint,
 ADD CONSTRAINT users_person_scoped_fk FOREIGN KEY (tenant_id, person_id) REFERENCES persons(tenant_id, id),
 ADD CONSTRAINT users_affiliation_scoped_fk FOREIGN KEY (tenant_id, affiliation_id, person_id, organisation_id) REFERENCES person_affiliations(tenant_id, id, person_id, organisation_id),
 ADD CONSTRAINT users_affiliation_needs_person CHECK (affiliation_id IS NULL OR person_id IS NOT NULL),
 ADD CONSTRAINT users_affiliation_names_organisation CHECK ((affiliation_id IS NULL) = (organisation_id IS NULL));
CREATE INDEX users_person_idx ON users(tenant_id, person_id) WHERE person_id IS NOT NULL;
CREATE INDEX users_affiliation_idx ON users(tenant_id, affiliation_id) WHERE affiliation_id IS NOT NULL;
CREATE INDEX users_organisation_idx ON users(tenant_id, organisation_id) WHERE organisation_id IS NOT NULL;

-- Bilimbi signs in by email alone: the address is unique platform-wide. The
-- demo seed deliberately gave tenant 2's manager the same address as tenant
-- 1's (party CONTRACT.md); that one known pair is resolved here explicitly by
-- renaming the tenant 2 account, never by merging. Any other duplicate stops
-- the migration so an operator resolves it on purpose.
UPDATE users SET email='manager@other.example.test'
 WHERE tenant_id=2 AND id=5 AND lower(email)='manager@example.test'
 AND EXISTS (SELECT 1 FROM users o WHERE o.tenant_id<>2 AND lower(o.email)='manager@example.test' AND o.deleted_at IS NULL);
DO $$
DECLARE duplicates text;
BEGIN
 SELECT string_agg(address, ', ' ORDER BY address) INTO duplicates
 FROM (SELECT lower(email) AS address FROM users WHERE deleted_at IS NULL GROUP BY lower(email) HAVING count(*) > 1) d;
 IF duplicates IS NOT NULL THEN
  RAISE EXCEPTION 'core/user 006: a login email must be unique across tenants; resolve these accounts explicitly before migrating: %', duplicates;
 END IF;
END $$;
CREATE UNIQUE INDEX users_login_email_global_idx ON users(lower(email)) WHERE deleted_at IS NULL;
