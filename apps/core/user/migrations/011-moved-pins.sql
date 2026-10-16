-- Z1: the contact directory moved from /persons to /contacts and the
-- relationship-role catalog from /companies/relationship-roles to
-- /companies/relationship-types (Bilimbi's names). The old addresses still
-- redirect, but a pin keeps the URL it was made on, so the new page would
-- show itself unpinned and pinning it again would hold one page twice. Each
-- pin moves to the new address, or goes when the account already pinned that
-- address; the directory's old page titles become its new ones.
CREATE TEMPORARY TABLE moved_pins ON COMMIT DROP AS
SELECT id, tenant_id, user_id,
       CASE WHEN url LIKE '/persons%' THEN '/contacts' || substr(url, 9)
            ELSE '/companies/relationship-types' || substr(url, 30) END AS url
  FROM user_pins
 WHERE url = '/persons' OR url LIKE '/persons/%' OR url LIKE '/persons?%'
    OR url = '/companies/relationship-roles' OR url LIKE '/companies/relationship-roles?%';

DELETE FROM user_pins p USING moved_pins m
 WHERE p.id = m.id
   AND EXISTS (SELECT 1 FROM user_pins o
                WHERE o.tenant_id = m.tenant_id AND o.user_id = m.user_id
                  AND o.url_hash = md5(m.url));

UPDATE user_pins p
   SET url = m.url, url_hash = md5(m.url), updated_at = now(),
       label = CASE p.label WHEN 'People' THEN 'Contacts'
                            WHEN 'New Person' THEN 'New contact'
                            ELSE p.label END
  FROM moved_pins m
 WHERE p.id = m.id;
