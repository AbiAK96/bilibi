-- Single-use nonce ledger of the reference webhook specimen. One row per
-- accepted delivery proves replay refusal at runtime; rows older than a day
-- are pruned on the next delivery, well past the signature freshness window.
-- No body is stored, only its length and SHA-256.
CREATE TABLE base_integration_reference_webhook (
 nonce text PRIMARY KEY CHECK (length(nonce) BETWEEN 1 AND 128),
 signed_at bigint NOT NULL,
 received_at timestamptz NOT NULL DEFAULT now(),
 body_bytes integer NOT NULL CHECK (body_bytes >= 0),
 body_sha256 text NOT NULL CHECK (length(body_sha256) = 64)
);
