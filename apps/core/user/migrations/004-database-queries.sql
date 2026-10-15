CREATE TABLE user_database_queries (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 tenant_id bigint NOT NULL,
 user_id bigint NOT NULL,
 name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 150),
 slug text NOT NULL CHECK (length(slug) BETWEEN 1 AND 200 AND slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
 prompt text NOT NULL DEFAULT '' CHECK (length(prompt) <= 100000),
 sql_query text NOT NULL CHECK (length(btrim(sql_query)) BETWEEN 1 AND 100000),
 description text NOT NULL DEFAULT '' CHECK (length(description) <= 100000),
 icon text NOT NULL DEFAULT '' CHECK (length(icon) <= 100),
 version bigint NOT NULL DEFAULT 1,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY (tenant_id,user_id) REFERENCES users(tenant_id,id),
 UNIQUE (tenant_id,user_id,slug)
);
CREATE INDEX user_database_queries_owner_idx ON user_database_queries(tenant_id,user_id,lower(name),id);
