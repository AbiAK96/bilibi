-- Geographic reference data (Bilimbi `Core.Geonames`, canonical Belimbing
-- tables). Global: no tenant column, no owner, no foreign key to anything but
-- the countries these rows belong to. Installation creates the tables and
-- NO rows; an operator fills them with the explicit import (C2/C3).
-- Bilimbi's quoted camelCase `"admin1Code"` PHP identifier is `admin1_code`
-- here. Timestamps are timestamptz like every other Bilibi table.
CREATE TABLE geonames_countries (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 iso text NOT NULL CHECK (iso ~ '^[A-Z]{2}$'),
 iso3 text NOT NULL CHECK (iso3 ~ '^[A-Z]{3}$'),
 iso_numeric text NOT NULL CHECK (length(iso_numeric) = 3),
 country text NOT NULL CHECK (length(country) BETWEEN 1 AND 255),
 capital text CHECK (length(capital) <= 255),
 area double precision,
 population bigint NOT NULL DEFAULT 0 CHECK (population >= 0),
 continent text NOT NULL CHECK (length(continent) = 2),
 tld text CHECK (length(tld) <= 3),
 currency_code text CHECK (length(currency_code) <= 3),
 currency_name text CHECK (length(currency_name) <= 32),
 phone text CHECK (length(phone) <= 24),
 postal_code_format text CHECK (length(postal_code_format) <= 100),
 postal_code_regex text,
 languages text CHECK (length(languages) <= 255),
 geoname_id integer,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CONSTRAINT geonames_countries_iso_unique UNIQUE (iso),
 CONSTRAINT geonames_countries_iso3_unique UNIQUE (iso3),
 CONSTRAINT geonames_countries_iso_numeric_unique UNIQUE (iso_numeric),
 CONSTRAINT geonames_countries_geoname_id_unique UNIQUE (geoname_id)
);
CREATE INDEX geonames_countries_continent_index ON geonames_countries (continent);

-- `code` is GeoNames' full `CC.value` identity. The country is derived once,
-- stored and indexed, so listing a country's divisions never scans the table.
CREATE TABLE geonames_admin1 (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 code text NOT NULL CHECK (length(code) BETWEEN 1 AND 20),
 country_iso text GENERATED ALWAYS AS (upper(split_part(code, '.', 1))) STORED,
 name text NOT NULL CHECK (length(name) BETWEEN 1 AND 255),
 alt_name text CHECK (length(alt_name) <= 255),
 geoname_id integer,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CONSTRAINT geonames_admin1_code_unique UNIQUE (code),
 CONSTRAINT geonames_admin1_geoname_id_unique UNIQUE (geoname_id)
);
CREATE INDEX geonames_admin1_country_iso_index ON geonames_admin1 (country_iso, name, code);

CREATE TABLE geonames_postcodes (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 country_iso text NOT NULL REFERENCES geonames_countries (iso) ON UPDATE CASCADE ON DELETE RESTRICT,
 postcode text NOT NULL CHECK (length(postcode) BETWEEN 1 AND 20),
 place_name text NOT NULL CHECK (length(place_name) BETWEEN 1 AND 180),
 admin1_code text CHECK (length(admin1_code) <= 20),
 admin_name1 text CHECK (length(admin_name1) <= 100),
 admin_code1 text CHECK (length(admin_code1) <= 20),
 admin_name2 text CHECK (length(admin_name2) <= 100),
 admin_code2 text CHECK (length(admin_code2) <= 20),
 admin_name3 text CHECK (length(admin_name3) <= 100),
 admin_code3 text CHECK (length(admin_code3) <= 20),
 latitude numeric(10, 7) CHECK (latitude BETWEEN -90 AND 90),
 longitude numeric(10, 7) CHECK (longitude BETWEEN -180 AND 180),
 accuracy smallint CHECK (accuracy BETWEEN 1 AND 6),
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX geonames_postcodes_country_iso_postcode_index ON geonames_postcodes (country_iso, postcode);
CREATE INDEX geonames_postcodes_country_iso_place_name_index ON geonames_postcodes (country_iso, place_name);
-- Address forms prefix-search a country's postcodes case-insensitively.
CREATE INDEX geonames_postcodes_prefix_index ON geonames_postcodes (country_iso, upper(postcode) text_pattern_ops);
CREATE INDEX geonames_postcodes_admin1_code_index ON geonames_postcodes (admin1_code);

CREATE TABLE geonames_cities (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 geoname_id integer NOT NULL,
 name text NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
 ascii_name text NOT NULL CHECK (length(ascii_name) BETWEEN 1 AND 200),
 alternate_names text,
 latitude numeric(10, 7) NOT NULL CHECK (latitude BETWEEN -90 AND 90),
 longitude numeric(10, 7) NOT NULL CHECK (longitude BETWEEN -180 AND 180),
 country_iso text NOT NULL REFERENCES geonames_countries (iso) ON UPDATE CASCADE ON DELETE RESTRICT,
 admin1_code text CHECK (length(admin1_code) <= 20),
 population bigint NOT NULL DEFAULT 0 CHECK (population >= 0),
 timezone text NOT NULL CHECK (length(timezone) BETWEEN 1 AND 40),
 modification_date date,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CONSTRAINT geonames_cities_geoname_id_unique UNIQUE (geoname_id)
);
CREATE INDEX geonames_cities_country_iso_index ON geonames_cities (country_iso, population DESC, geoname_id);
CREATE INDEX geonames_cities_admin1_code_index ON geonames_cities (country_iso, admin1_code);
CREATE INDEX geonames_cities_timezone_index ON geonames_cities (timezone);
