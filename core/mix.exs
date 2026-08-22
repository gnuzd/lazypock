defmodule Lazypock.MixProject do
  use Mix.Project

  def project do
    [
      app: :lazypock,
      version: "0.4.3", # x-release-please-version
      license: "MIT",
      elixir: "~> 1.17",
      elixirc_paths: elixirc_paths(Mix.env()),
      start_permanent: Mix.env() == :prod,
      aliases: aliases(),
      deps: deps(),
      compilers: Mix.compilers(),
      listeners: [Phoenix.CodeReloader],
      releases: releases()
    ]
  end

  # Configuration for the OTP application.
  #
  # Type `mix help compile.app` for more information.
  def application do
    [
      mod: {Lazypock.Application, []},
      extra_applications: [:logger, :runtime_tools, :inets, :ssl]
    ]
  end

  def cli do
    [
      preferred_envs: [precommit: :test]
    ]
  end

  def releases do
    [
      lazypock: [
        steps: [:assemble, &Burrito.wrap/1],
        burrito: [
          # Burrito plugin: clears stale cached install dirs on launch so a new
          # binary always re-extracts fresh code (migrate! runs on first boot).
          plugin: "burrito_plugin.zig",
          targets: [
            # macos: [os: :darwin, cpu: :x86_64],
            macos_silicon: [os: :darwin, cpu: :aarch64],
            linux: [os: :linux, cpu: :x86_64]
            # windows: [os: :windows, cpu: :x86_64]
          ]
        ]
      ]
    ]
  end

  # Specifies which paths to compile per environment.
  defp elixirc_paths(:test), do: ["lib", "test/support"]
  defp elixirc_paths(_), do: ["lib", "priv/hooks"]

  # Specifies your project dependencies.
  #
  # Type `mix help deps` for examples and options.
  defp deps do
    [
      {:phoenix, "~> 1.8.9"},
      {:phoenix_ecto, "~> 4.5"},
      {:ecto_sql, "~> 3.13"},
      {:postgrex, ">= 0.0.0"},
      {:phoenix_html, "~> 4.1"},
      {:telemetry_metrics, "~> 1.0"},
      {:telemetry_poller, "~> 1.0"},
      {:jason, "~> 1.2"},
      {:dns_cluster, "~> 0.2.0"},
      {:bandit, "~> 1.5"},
      {:bcrypt_elixir, "~> 3.0"},
      {:burrito, "~> 1.0"},
      {:swoosh, "~> 1.17"},
      {:gen_smtp, "~> 1.2"},
      {:cors_plug, "~> 3.0"},
      {:assent, "~> 0.3.1"},
      {:crontab, "~> 1.2"},
      {:tz, "~> 0.28"}
    ]
  end

  # Aliases are shortcuts or tasks specific to the current project.
  # For example, to install project dependencies and perform other setup tasks, run:
  #
  #     $ mix setup
  #
  # See the documentation for `Mix` for more info on aliases.
  defp aliases do
    [
      setup: ["deps.get", "ecto.setup", "assets.setup", "assets.build"],
      "ecto.setup": ["ecto.create", "ecto.migrate", "run priv/repo/seeds.exs"],
      "ecto.reset": ["ecto.drop", "ecto.setup"],
      test: ["ecto.create --quiet", "ecto.migrate --quiet", "test"],
      "assets.setup": fn _ ->
        IO.puts("Studio UI: cd studio && bun install")
      end,
      "assets.build": ["compile", "studio.build"],
      "studio.build": fn _ ->
        {_, 0} = System.cmd("bun", ["run", "build"], cd: "../studio", into: IO.binstream())
      end,
      "assets.deploy": [
        "studio.build",
        "phx.digest"
      ],
      precommit: ["compile --warnings-as-errors", "deps.unlock --unused", "format", "test"]
    ]
  end
end
