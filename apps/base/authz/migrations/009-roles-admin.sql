-- Role administration (P7): when a role began, for the roles list's created_at
-- sort. Existing rows take the migration time.
ALTER TABLE roles ADD COLUMN created_at timestamptz NOT NULL DEFAULT now();
