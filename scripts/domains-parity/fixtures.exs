# Run with mix run --no-start /absolute/path/to/fixtures.exs in the exported
# reference. This command provisions only the disposable parity database.
# Domain data remains empty; extend walkthrough.exs through public facades.
expected = "bilimbi_domains_parity"
url = System.fetch_env!("DATABASE_URL") |> URI.parse()
if url.host != "127.0.0.1" or url.path != "/" <> expected,
  do: raise("Fixtures require the private loopback #{expected} database")

root = Path.dirname(__ENV__.file)
identity = root |> Path.join("identity.json") |> File.read!() |> Jason.decode!()
BilimbiWeb.Release.seed()
{:ok, _status} = Bilimbi.Core.User.bootstrap_platform_admin(%{
  tenant_name: "Parity reference",
  company_name: "Parity reference",
  company_code: "PARITY",
  admin_name: "Parity operator",
  admin_email: identity["email"],
  password: identity["password"]
})
{:ok, user} = Bilimbi.Core.User.authenticate(identity["email"], identity["password"])
{:ok, tenant_id} = Bilimbi.Core.Company.fetch_tenant_id_for_company(user.company_id)
manifest = %{tenant_id: tenant_id, company_id: user.company_id, user_id: user.id,
             state: "empty", domain_records_created: 0}
File.write!(Path.join(root, "fixture-state.json"), Jason.encode!(manifest, pretty: true))
IO.puts("Disposable reference identity provisioned; all domain tables remain empty")
