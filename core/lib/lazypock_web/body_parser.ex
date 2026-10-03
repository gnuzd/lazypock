defmodule LazypockWeb.BodyParser do
  @moduledoc """
  Path-aware replacement for the endpoint's `plug Plug.Parsers`.

  `Plug.Parsers`' default request-body cap is 8 MB, which is too small both for
  an NDJSON backup archive and for a legitimate large image upload. Raising it
  with the global `:length` option would relax *every* endpoint, so this
  dispatches between three precompiled configurations:

    * `/api/import` — `LAZYPOCK_IMPORT_MAX_MB` (default 10240 = 10 GB)
    * `POST /api/files` — `LAZYPOCK_UPLOAD_MAX_MB` (default 20), the upload hard max
    * everything else — the 8 MB default

  The caps live here rather than in the router because endpoint plugs run before
  routing, so a router pipeline cannot scope them.

  The environment is read on first use, not at compile time, so the single
  binary stays configurable at runtime.
  """

  @behaviour Plug

  @base [
    parsers: [:urlencoded, :multipart, :json],
    pass: ["*/*"],
    json_decoder: Phoenix.json_library()
  ]

  # Plug.Parsers' own default — kept explicit so the intent is visible.
  @default_length 8_000_000
  @default_upload_mb 20
  @archive_import_path "/api/import"
  @file_upload_path "/api/files"

  @impl true
  def init(opts) do
    base = Keyword.merge(@base, opts)

    %{default: Plug.Parsers.init(Keyword.put_new(base, :length, @default_length))}
  end

  @impl true
  def call(conn, %{default: default}) do
    parser =
      cond do
        archive_import?(conn) -> archive_parser()
        file_upload?(conn) -> upload_parser()
        true -> default
      end

    Plug.Parsers.call(conn, parser)
  end

  defp archive_import?(%Plug.Conn{method: "POST", request_path: @archive_import_path}), do: true
  defp archive_import?(_conn), do: false

  defp file_upload?(%Plug.Conn{method: "POST", request_path: @file_upload_path}), do: true
  defp file_upload?(_conn), do: false

  # Precompiling Plug.Parsers is comparatively expensive, so each variant is
  # memoized keyed by the configured length (it is rebuilt if the limit changes,
  # which only happens if the env var does).
  defp archive_parser do
    length = archive_length()

    case :persistent_term.get({__MODULE__, :archive}, nil) do
      {^length, parser} ->
        parser

      _ ->
        parser = Plug.Parsers.init(Keyword.put(@base, :length, length))
        :persistent_term.put({__MODULE__, :archive}, {length, parser})
        parser
    end
  end

  defp upload_parser do
    length = upload_length()

    case :persistent_term.get({__MODULE__, :upload}, nil) do
      {^length, parser} ->
        parser

      _ ->
        parser = Plug.Parsers.init(Keyword.put(@base, :length, length))
        :persistent_term.put({__MODULE__, :upload}, {length, parser})
        parser
    end
  end

  defp archive_length do
    case Integer.parse(System.get_env("LAZYPOCK_IMPORT_MAX_MB") || "") do
      {mb, _rest} when mb > 0 -> mb * 1_048_576
      _ -> 10_240 * 1_048_576
    end
  end

  defp upload_length do
    case Integer.parse(System.get_env("LAZYPOCK_UPLOAD_MAX_MB") || "") do
      {mb, _rest} when mb > 0 -> mb * 1_048_576
      _ -> @default_upload_mb * 1_048_576
    end
  end
end
