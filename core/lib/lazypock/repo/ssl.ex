defmodule Lazypock.Repo.SSL do
  @moduledoc """
  Translates libpq-style TLS connection parameters into the `:ssl` option
  Postgrex actually understands.

  Ecto's URL parser (`Ecto.Repo.Supervisor.parse_url/1`) recognizes only
  `?ssl=true|false` and a handful of integer parameters. Every other query
  parameter — `sslmode` included — is passed through to Postgrex verbatim,
  where it is silently ignored. A `DATABASE_URL` ending in
  `?sslmode=require` therefore used to connect in **plaintext**.

  This module reads `sslmode` (and `sslrootcert` / `sslcert` / `sslkey`) from
  the repo `:url` query string, an explicit config key, or `PGSSLMODE`, and
  maps it onto Postgrex's `:ssl` option using libpq semantics:

  | `sslmode`     | Postgrex `:ssl`                                            |
  |---------------|------------------------------------------------------------|
  | `disable`     | `false` — no TLS                                           |
  | `require`     | `[verify: :verify_none]` (encrypt; verify only if `sslrootcert` is set) |
  | `verify-ca`   | `verify_peer` against the CA, hostname **not** checked     |
  | `verify-full` | `verify_peer` against the CA, hostname checked             |

  `allow` and `prefer` are rejected: Postgrex has no opportunistic-TLS
  fallback, and silently downgrading to plaintext is exactly the failure this
  module exists to prevent. An unrecognized `sslmode` value is rejected too.

  An explicit `:ssl` option in the repo config always wins over the URL.
  """

  require Logger

  @params [
    {"sslmode", :sslmode},
    {"sslrootcert", :sslrootcert},
    {"sslcert", :sslcert},
    {"sslkey", :sslkey},
    {"channel_binding", :channel_binding}
  ]

  @doc """
  Returns the repo config with `:ssl` derived from the URL's TLS parameters.

  Configs without any TLS parameter are returned unchanged, so this is safe
  to call for every environment.
  """
  @spec apply(keyword()) :: keyword()
  def apply(config) do
    params = params(config)
    maybe_warn_channel_binding(params)

    mode = sslmode(params)

    cond do
      is_nil(mode) ->
        config

      Keyword.has_key?(config, :ssl) ->
        Logger.warning(
          "sslmode=#{mode} in the connection URL is ignored because an explicit " <>
            ":ssl option is already configured for the repo"
        )

        config

      true ->
        Keyword.put(config, :ssl, ssl_opts(mode, params))
    end
  end

  # ── parameters ──────────────────────────────────────────────

  defp params(config) do
    url_params =
      case config[:url] do
        url when is_binary(url) -> decode_query(url)
        _ -> %{}
      end

    config_params =
      for {param, key} <- @params,
          value = config[key],
          is_binary(value),
          into: %{},
          do: {param, value}

    Map.merge(url_params, config_params)
  end

  defp decode_query(url) do
    url |> URI.parse() |> Map.get(:query) |> URI.decode_query()
  rescue
    _ -> %{}
  end

  defp sslmode(params) do
    params["sslmode"] || System.get_env("PGSSLMODE")
  end

  # ── mapping ─────────────────────────────────────────────────

  defp ssl_opts(mode, params) do
    case mode |> to_string() |> String.trim() |> String.downcase() do
      "disable" ->
        false

      preference when preference in ["allow", "prefer"] ->
        raise ArgumentError,
              "unsupported sslmode=#{preference}: Postgrex has no opportunistic TLS " <>
                "fallback, so it cannot prefer TLS and downgrade to plaintext. " <>
                "Use sslmode=require to require TLS, or sslmode=disable to allow plaintext."

      "require" ->
        if present?(params["sslrootcert"]) do
          # libpq verifies the certificate when a root CA is available, even
          # at `require`; without one it only encrypts.
          verify_opts(params, hostname: false)
        else
          [verify: :verify_none] ++ client_cert_opts(params)
        end

      "verify-ca" ->
        verify_opts(params, hostname: false)

      "verify-full" ->
        verify_opts(params, hostname: true)

      other ->
        raise ArgumentError,
              "unsupported sslmode=#{inspect(other)}: expected one of " <>
                "disable, allow, prefer, require, verify-ca, verify-full"
    end
  end

  defp verify_opts(params, hostname: hostname?) do
    opts = [verify: :verify_peer] ++ ca_opts(params) ++ client_cert_opts(params)

    if hostname? do
      opts
    else
      # `verify-ca` verifies the certificate chain but not the hostname. Postgrex
      # defaults add a hostname check (`customize_hostname_check`), so override
      # it with a permissive match function to match libpq's behavior.
      opts ++ [customize_hostname_check: [match_fun: fn _reference, _presented -> true end]]
    end
  end

  defp ca_opts(params) do
    if present?(params["sslrootcert"]) do
      [cacertfile: String.to_charlist(params["sslrootcert"])]
    else
      [cacerts: system_cacerts()]
    end
  end

  defp client_cert_opts(params) do
    for {param, opt} <- [{"sslcert", :certfile}, {"sslkey", :keyfile}],
        present?(params[param]),
        do: {opt, String.to_charlist(params[param])}
  end

  defp system_cacerts do
    case :public_key.cacerts_get() do
      certs when is_list(certs) and certs != [] ->
        certs

      _ ->
        raise ArgumentError,
              "could not load the system CA trust store for sslmode=verify-ca/verify-full; " <>
                "set sslrootcert=<path to CA bundle> in DATABASE_URL"
    end
  rescue
    e in ArgumentError ->
      reraise e, __STACKTRACE__

    e ->
      raise ArgumentError,
            "could not load the system CA trust store (#{Exception.message(e)}); " <>
              "set sslrootcert=<path to CA bundle> in DATABASE_URL"
  end

  defp maybe_warn_channel_binding(params) do
    if params["channel_binding"] in ["require", "required"] do
      Logger.warning(
        "channel_binding=require is ignored: Postgrex does not implement SCRAM-SHA-256-PLUS, " <>
          "so the connection is authenticated with SCRAM-SHA-256 but without channel binding. " <>
          "The connection is still encrypted according to sslmode."
      )
    end
  end

  defp present?(value), do: is_binary(value) and String.trim(value) != ""
end
