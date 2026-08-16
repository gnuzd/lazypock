# This file is responsible for configuring your application
# and its dependencies with the aid of the Config module.
#
# This configuration file is loaded before any dependency and
# is restricted to this project.

# General application configuration
import Config

config :lazypock,
  ecto_repos: [Lazypock.Repo],
  generators: [timestamp_type: :utc_datetime]

# Configure the endpoint
config :lazypock, LazypockWeb.Endpoint,
  url: [host: "localhost"],
  adapter: Bandit.PhoenixAdapter,
  render_errors: [
    formats: [json: LazypockWeb.ErrorJSON],
    layout: false
  ],
  pubsub_server: Lazypock.PubSub

# Configure Elixir's Logger
config :logger, :default_formatter,
  format: "$time $metadata[$level] $message\n",
  metadata: [:request_id]

# Use Jason for JSON parsing in Phoenix
config :phoenix, :json_library, Jason

# Plugs must init at RUNTIME (not compile) so CORSPlug.init/1 can read
# `config :cors_plug` from runtime.exs at boot (env-driven origins).
# Without this, CORS origins are baked into the beam at build time.
config :phoenix, :plug_init_mode, :runtime

# Swoosh mailer config
config :lazypock, Lazypock.Mailer, adapter: Swoosh.Adapters.Test

# Disable Swoosh's default API client (we only use SMTP)
config :swoosh, :api_client, false

# OAuth2 providers (PocketBase parity). Each provider is a map with
# `client_id`, `client_secret`, and optional `authorize_url` / `token_url` /
# `user_url` / `scope` overrides for generic providers.
#
# Providers are also configurable at runtime via the admin Settings UI
# (Lazypock.Settings) under the "oauth2.providers" key, which takes
# precedence over this static config.
config :lazypock, :oauth2_providers, []

# Import environment specific config. This must remain at the bottom
# of this file so it overrides the configuration defined above.
import_config "#{config_env()}.exs"
