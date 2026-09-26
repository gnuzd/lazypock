[
  import_deps: [:ecto, :ecto_sql, :phoenix],
  subdirectories: ["priv/*/migrations"],
  # NOTE: `mix.exs` is deliberately absent from `inputs`. The Elixir 1.20
  # formatter moves the `# x-release-please-version` annotation onto its own
  # line, and release-please needs it on the `version:` line to locate the
  # version it bumps. Formatting mix.exs by hand would silently break releases.
  inputs: ["{config,lib,test}/**/*.{ex,exs}", "priv/*/seeds.exs"]
]
