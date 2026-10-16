-- K16: the account page's facts and the stale-edit check its inline editors
-- carry back. `version` moves on every administrative write of the row, so a
-- draft begun before another operator's save is refused rather than applied
-- over it; the timestamps are Bilimbi's Created and Updated facts.
ALTER TABLE users
 ADD COLUMN version bigint NOT NULL DEFAULT 1,
 ADD COLUMN created_at timestamptz NOT NULL DEFAULT now(),
 ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now();
