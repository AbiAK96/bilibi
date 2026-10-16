# Skeleton for later slice proofs, run against a SEPARATE disposable database.
# Do not run populated walkthroughs on the preserved empty-state reference.
# Copy the identity/bootstrap steps from fixtures.exs into the new database,
# authenticate it, prove company membership, then seal that actor at this edge.
# All business writes below use domain facades, never Ecto inserts or raw SQL.

# alias Bilimbi.Base.Tenancy
# alias Bilimbi.Base.Tenancy.Authentication
# alias Bilimbi.Core.{Company, User}
# alias Bilimbi.SupplyChain.{Purchasing, Receiving}
# alias Bilimbi.Finance.{Ledger, Payables, Receivables, Reconciliation, Sst}
# alias Bilimbi.Factory.{Inventory, ProductDefinition, ProductionExecution}
#
# {:ok, user} = User.authenticate(email, password)
# {:ok, tenant_id} = Company.fetch_tenant_id_for_company(user.company_id)
# {:ok, system_scope} = Tenancy.scope(tenant_id)
# {:ok, _user} = User.get_user(system_scope, user.company_id, user.id)
# scope = Authentication.sign_in(system_scope, user.id, user.company_id)
# company_id = user.company_id
#
# Supply Chain logical record keys:
#   supplier, purchase_request, purchase_order, receipt
# Finance logical record keys:
#   book, cash_account, expense_account, payable_account, receivable_account,
#   tax_code, customer, bill, invoice, bank_account, statement, reconciliation,
#   asset_category, asset, depreciation_run, einvoice_document, sst_return
# Factory logical record keys:
#   native_unit, location, material_type, material, resource_type, resource,
#   product, formula_revision, routing_revision, production_order, execution
#
# Example facade boundary (only when the proof requires an open book):
# {:ok, book} = Ledger.open_book(scope, company_id,
#   %{currency_code: "MYR", minor_units: 2})
# {:ok, cash} = Ledger.create_account(scope, company_id,
#   %{code: "1000", name: "Cash", account_type: "asset"})
#
# Persist only returned IDs and logical keys to walkthrough-state.json. Build
# the Rust walkthrough with the SAME logical keys, amounts, dates, statuses,
# units, and actor grants. Never assert equal auto-generated database IDs.
# Record each interaction's starting state, expected refusal/success, affected
# logical keys, and the actual returned outcome. Never swallow a facade error.
raise "Walkthrough skeleton: select a slice and implement its facade sequence first"
