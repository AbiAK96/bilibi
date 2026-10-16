CREATE TABLE port_reference_order_lines(
  id bigserial PRIMARY KEY,
  tenant_id bigint NOT NULL,
  company_id bigint NOT NULL,
  order_number text NOT NULL,
  quantity numeric(20,4) NOT NULL,
  unit_price numeric(20,4) NOT NULL,
  currency_code text NOT NULL,
  status text NOT NULL
);
