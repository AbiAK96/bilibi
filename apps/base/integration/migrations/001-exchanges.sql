-- One general log of incoming and outgoing HTTP exchanges (Bilimbi
-- `base_integration_outbound_exchanges` plus its general-log columns). A row is
-- a machine measurement: no foreign keys, so evidence outlives tenants and
-- accounts. A null tenant is installation-wide system traffic or a call whose
-- credential resolved no tenant; only the platform operator may see it.
-- Payload columns hold redacted, bounded previews and only when the caller
-- opted in; retention clears them and keeps the metadata.
CREATE TABLE base_integration_exchanges (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 public_id text NOT NULL UNIQUE CHECK (length(public_id) BETWEEN 1 AND 40),
 tenant_id bigint,
 direction text NOT NULL CHECK (direction IN ('incoming', 'outgoing')),
 system text NOT NULL CHECK (length(system) BETWEEN 1 AND 255),
 provider text CHECK (length(provider) <= 255),
 operation text NOT NULL CHECK (length(operation) BETWEEN 1 AND 255),
 transport text NOT NULL DEFAULT 'http' CHECK (length(transport) BETWEEN 1 AND 40),
 protocol text NOT NULL DEFAULT 'rest' CHECK (length(protocol) BETWEEN 1 AND 40),
 endpoint text NOT NULL CHECK (length(endpoint) BETWEEN 1 AND 4096),
 method text CHECK (length(method) BETWEEN 1 AND 16),
 host text CHECK (length(host) <= 255),
 path text NOT NULL CHECK (length(path) BETWEEN 1 AND 4096),
 owner_type text CHECK (length(owner_type) <= 255),
 owner_id bigint,
 correlation_id text CHECK (length(correlation_id) <= 255),
 request_headers jsonb,
 request_body jsonb,
 response_headers jsonb,
 response_body jsonb,
 response_status smallint CHECK (response_status BETWEEN 100 AND 599),
 duration_ms integer CHECK (duration_ms >= 0),
 outcome text NOT NULL CHECK (outcome IN ('success', 'http_error', 'connection_error', 'unknown')),
 error_class text CHECK (length(error_class) <= 255),
 error_message text CHECK (length(error_message) <= 255),
 user_id bigint,
 impersonator_id bigint,
 connection_id text CHECK (length(connection_id) <= 255),
 agent_id text CHECK (length(agent_id) <= 255),
 run_id text CHECK (length(run_id) <= 255),
 occurred_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX base_integration_exchanges_timeline_idx ON base_integration_exchanges (tenant_id, direction, occurred_at DESC, id DESC);
CREATE INDEX base_integration_exchanges_occurred_idx ON base_integration_exchanges (occurred_at);
CREATE INDEX base_integration_exchanges_owner_idx ON base_integration_exchanges (owner_type, owner_id);
CREATE INDEX base_integration_exchanges_system_idx ON base_integration_exchanges (system, operation);
CREATE INDEX base_integration_exchanges_correlation_idx ON base_integration_exchanges (correlation_id);
