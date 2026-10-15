-- Payload-free client facts the operator list shows (Bilimbi `sessions`
-- carries `ip_address` and `user_agent`). Recorded once at sign-in; null when
-- the request named neither.
ALTER TABLE sessions ADD COLUMN ip_address text, ADD COLUMN user_agent text;
CREATE INDEX sessions_last_activity_idx ON sessions(last_activity DESC, id);
