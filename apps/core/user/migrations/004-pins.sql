-- C5: the account's sidebar pins. Keyed by the current tenant + account IDs, so
-- membership is untouched and a later account/person binding (K9) moves no
-- pin. One row per normalized URL per account (`url_hash` is md5 of the
-- normalized URL); display order is `sort_order`, then `id`.
CREATE TABLE user_pins (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 tenant_id bigint NOT NULL, user_id bigint NOT NULL,
 label text NOT NULL CHECK (char_length(label) BETWEEN 1 AND 150),
 url text NOT NULL CHECK (char_length(url) BETWEEN 1 AND 500),
 url_hash text NOT NULL CHECK (url_hash ~ '^[0-9a-f]{32}$'),
 icon text CHECK (icon IS NULL OR char_length(icon) <= 100),
 sort_order integer NOT NULL DEFAULT 0,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(tenant_id,user_id) REFERENCES users(tenant_id,id) ON DELETE CASCADE,
 UNIQUE(tenant_id,user_id,url_hash)
);
CREATE INDEX user_pins_order_idx ON user_pins(tenant_id,user_id,sort_order,id);
