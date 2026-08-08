defmodule Lazypock do
  @moduledoc """
  Lazypock keeps the contexts that define your domain
  and business logic.
  """

  @doc """
  Returns the current app context (the application module).
  Mirrors PocketBase's `$app` reference available in every hook event.
  """
  def app, do: __MODULE__
end
