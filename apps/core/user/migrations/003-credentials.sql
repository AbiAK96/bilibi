-- C7: email verification state and bounded password-reset token storage.
ALTER TABLE users ADD COLUMN email_verified_at timestamptz;

-- At most one live reset token per account, so storage is bounded by the user
-- count. Only the SHA-256 of the 256-bit random token is stored; the plaintext
-- exists only in the delivered link.
CREATE TABLE password_reset_tokens (
 tenant_id bigint NOT NULL, user_id bigint NOT NULL,
 token_hash bytea NOT NULL CHECK (octet_length(token_hash)=32),
 created_at timestamptz NOT NULL DEFAULT now(),
 expires_at timestamptz NOT NULL CHECK (expires_at > created_at),
 PRIMARY KEY(tenant_id,user_id),
 FOREIGN KEY(tenant_id,user_id) REFERENCES users(tenant_id,id) ON DELETE CASCADE
);
CREATE INDEX password_reset_tokens_expiry_idx ON password_reset_tokens(expires_at);
CREATE INDEX users_login_email_idx ON users(tenant_id,lower(email));
