-- Bilimbi's notification shape (`Core.User.Notification`): a type plus the
-- title/body/url/icon payload the bell and /notifications render. The former
-- one-line message becomes the title.
ALTER TABLE notifications RENAME COLUMN message TO title;
ALTER TABLE notifications
 ADD COLUMN type text NOT NULL DEFAULT 'generic',
 ADD COLUMN body text NOT NULL DEFAULT '',
 ADD COLUMN url text,
 ADD COLUMN icon text,
 ADD CONSTRAINT notifications_title_bounded CHECK (length(title) BETWEEN 1 AND 255),
 ADD CONSTRAINT notifications_body_bounded CHECK (length(body) <= 2000),
 ADD CONSTRAINT notifications_url_relative CHECK (url IS NULL OR (url ~ '^/' AND url !~ '^//' AND url !~ '\\' AND length(url) <= 2048));
CREATE INDEX notifications_recipient_recent_idx ON notifications(tenant_id,user_id,created_at DESC,id DESC);
