defmodule LazypockWeb.AdminChannel do
  @moduledoc """
  Phoenix Channel for admin-level realtime events.

  Topics:
    `collections` — collection CRUD (create/update/delete) events
  """

  use Phoenix.Channel

  @impl true
  def join("collections", _payload, socket) do
    # Require authenticated superuser
    case socket.assigns[:current_user] do
      %{"role" => "superuser"} ->
        {:ok, socket}

      %{} ->
        {:error, %{reason: "Access denied. Superuser required."}}

      nil ->
        {:error, %{reason: "Authentication required."}}
    end
  end

  @impl true
  def join(_topic, _payload, _socket) do
    {:error, %{reason: "Unknown topic."}}
  end
end
