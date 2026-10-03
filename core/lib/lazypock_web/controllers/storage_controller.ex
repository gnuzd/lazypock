defmodule LazypockWeb.StorageController do
  @moduledoc """
  S3/R2 storage configuration (superuser only).

  The secret access key is encrypted at rest and never returned; `GET` shows a
  mask plus `secret_set`, and `configured_from_env` lists the fields locked by
  environment variables (env wins over the Studio).
  """

  use LazypockWeb, :controller

  alias Lazypock.Files.Adapters.S3
  alias Lazypock.Files.Storage
  alias Lazypock.Settings

  def show(conn, _params) do
    conn = require_superuser!(conn)
    if conn.halted, do: conn, else: json(conn, Storage.public_view())
  end

  def update(conn, params) do
    conn = require_superuser!(conn)

    if conn.halted do
      conn
    else
      do_update(conn, incoming(params))
    end
  end

  @doc "Runs a real PUT/HEAD/GET/DELETE round-trip against the bucket."
  def test(conn, _params) do
    conn = require_superuser!(conn)

    if conn.halted do
      conn
    else
      steps = S3.test_connection()
      ok? = Enum.all?(steps, & &1.ok)
      conn |> put_status(if(ok?, do: 200, else: 422)) |> json(%{"ok" => ok?, "steps" => steps})
    end
  end

  defp do_update(conn, incoming) do
    stored =
      case Settings.get("storage", %{}) do
        map when is_map(map) -> map
        _ -> %{}
      end

    merged =
      stored
      |> Storage.merge(incoming)
      |> Map.drop(["secret_set", "configured_from_env", "secret_error"])

    case Storage.validate(merged) do
      :ok ->
        Settings.put(Map.put(Settings.get(), "storage", merged))
        Storage.clear_cache()
        json(conn, Storage.public_view())

      {:error, message} ->
        conn
        |> put_status(422)
        |> json(%{"code" => 422, "message" => message, "data" => %{}})
    end
  end

  defp incoming(params) do
    case params["storage"] do
      map when is_map(map) -> map
      _ -> Map.drop(params, ["action", "controller", "_method", "_csrf_token"])
    end
  end

  defp require_superuser!(conn) do
    case conn.assigns[:current_superuser] do
      nil ->
        conn
        |> put_resp_content_type("application/json")
        |> send_resp(
          403,
          Jason.encode!(%{code: 403, message: "Access denied. Superuser required.", data: %{}})
        )
        |> halt()

      _user ->
        conn
    end
  end
end
