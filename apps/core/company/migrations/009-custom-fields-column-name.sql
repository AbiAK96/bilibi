ALTER TABLE companies RENAME COLUMN custom TO custom_fields;
ALTER TABLE companies
  RENAME CONSTRAINT companies_custom_check TO companies_custom_fields_check;
ALTER INDEX companies_custom_gin RENAME TO companies_custom_fields_gin;
