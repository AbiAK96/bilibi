# Loopback HTTP reference host. No shared ingress registration or worker jobs.
# Normal production configuration in the pinned sources is unchanged.
BilimbiWeb.Release.start_without_workers()
port = System.get_env("PARITY_HTTP_PORT", "41461") |> String.to_integer()
config = Application.fetch_env!(:web, BilimbiWeb.Endpoint)
config = Keyword.merge(config,
  url: [scheme: "http", host: "127.0.0.1", port: port],
  http: [ip: {127, 0, 0, 1}, port: port],
  check_origin: ["http://127.0.0.1:#{port}"],
  force_ssl: false,
  code_reloader: false,
  watchers: [],
  server: true
)
Application.put_env(:web, BilimbiWeb.Endpoint, config)
{:ok, _started} = Application.ensure_all_started(:web)
IO.puts("Parity reference ready: http://127.0.0.1:#{port}")
Process.sleep(:infinity)
