INSERT INTO principal_roles(tenant_id,user_id,role_id) VALUES(1,1,1),(1,2,2),(1,3,3),(1,4,4),(2,5,5),(1,6,1) ON CONFLICT DO NOTHING;
SELECT setval(pg_get_serial_sequence('users','id'),GREATEST((SELECT max(id) FROM users),1));
