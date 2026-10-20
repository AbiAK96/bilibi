-- SMS as an alternative proof for Core User's existing step-up provider.
-- Codes are Argon2id hashes; the plaintext exists only in the outbound SMS.
ALTER TABLE users
 ADD COLUMN phone_number text,
 ADD COLUMN phone_verified_at timestamptz,
 ADD CONSTRAINT users_phone_e164_check CHECK (
   phone_number IS NULL OR phone_number ~ '^\+[1-9][0-9]{7,14}$'
 ),
 ADD CONSTRAINT users_phone_verified_check CHECK (
   phone_verified_at IS NULL OR phone_number IS NOT NULL
 );

CREATE TABLE user_sms_challenges (
 id text PRIMARY KEY,
 tenant_id bigint NOT NULL,
 user_id bigint NOT NULL,
 purpose text NOT NULL CHECK (length(purpose) BETWEEN 1 AND 128),
 target text NOT NULL CHECK (length(target) BETWEEN 1 AND 512),
 phone_number text NOT NULL CHECK (phone_number ~ '^\+[1-9][0-9]{7,14}$'),
 code_hash text NOT NULL,
 attempts smallint NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 5),
 status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','sent','used','invalid')),
 created_at timestamptz NOT NULL DEFAULT now(),
 sent_at timestamptz,
 expires_at timestamptz NOT NULL,
 used_at timestamptz,
 CONSTRAINT user_sms_challenges_user_fkey FOREIGN KEY(tenant_id,user_id)
   REFERENCES users(tenant_id,id) ON DELETE CASCADE
);
CREATE INDEX user_sms_challenges_owner_idx
 ON user_sms_challenges(tenant_id,user_id,purpose,created_at DESC);
CREATE INDEX user_sms_challenges_expiry_idx
 ON user_sms_challenges(expires_at) WHERE status IN ('pending','sent');

-- The browser receives only a random handle. The real session token stays on
-- the server until the second factor succeeds.
CREATE TABLE user_pending_logins (
 handle_hash text PRIMARY KEY,
 session_id text NOT NULL UNIQUE REFERENCES sessions(id) ON DELETE CASCADE,
 created_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL
);
CREATE INDEX user_pending_logins_expiry_idx ON user_pending_logins(expires_at);
