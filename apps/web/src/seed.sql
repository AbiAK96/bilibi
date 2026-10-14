INSERT INTO tenants(id,name) VALUES(1,'Bilimbi Labs'),(2,'Other Workspace') ON CONFLICT DO NOTHING;
INSERT INTO roles(id,tenant_id,name,grant_all) VALUES(1,1,'manager',false),(2,1,'viewer',false),(3,1,'staff',false),(4,1,'admin',true),(5,2,'manager',false) ON CONFLICT DO NOTHING;
INSERT INTO role_capabilities(tenant_id,role_id,capability)
SELECT tenant_id,id,capability FROM roles CROSS JOIN unnest(ARRAY['companies.view','notifications.view','notifications.update','session.logout']) capability WHERE id IN (1,2,3,4,5) ON CONFLICT DO NOTHING;
INSERT INTO role_capabilities(tenant_id,role_id,capability) VALUES(1,1,'companies.update'),(1,3,'companies.update'),(2,5,'companies.update') ON CONFLICT DO NOTHING;
INSERT INTO field_restrictions(tenant_id,role_id,table_id,field_id)
SELECT 1,role_id,'companies',field_id FROM unnest(ARRAY[2,3]::bigint[]) role_id CROSS JOIN unnest(ARRAY['tax_id','revenue']) field_id ON CONFLICT DO NOTHING;
INSERT INTO field_restrictions(tenant_id,role_id,table_id,field_id) VALUES(1,2,'companies','email'),(1,2,'companies','phone') ON CONFLICT DO NOTHING;
INSERT INTO companies(id,tenant_id,name,status,email,phone,tax_id,revenue)
SELECT n,1,'Company '||lpad(n::text,3,'0'),CASE WHEN n%3=0 THEN 'pending' ELSE 'active' END,'contact'||n||'@example.test','+60 12 345 '||n,'TAX-'||n,125000+n FROM generate_series(1,62) n ON CONFLICT DO NOTHING;
INSERT INTO companies(id,tenant_id,name,status,email,phone,tax_id,revenue) VALUES(100,2,'Private other-tenant company','active','private@example.test','private phone','PRIVATE-TAX',987654321) ON CONFLICT DO NOTHING;
SELECT setval(pg_get_serial_sequence('tenants','id'),GREATEST((SELECT max(id) FROM tenants),1));
SELECT setval(pg_get_serial_sequence('roles','id'),GREATEST((SELECT max(id) FROM roles),1));
SELECT setval(pg_get_serial_sequence('companies','id'),GREATEST((SELECT max(id) FROM companies),1));
