defmodule Lazypock.Auth.OAuth2 do
  @moduledoc """
  OAuth2 authentication provider support (PocketBase parity).

  Implements the PocketBase `authWithOAuth2` flow:

    * provider configuration (static via `config :lazypock, :oauth2_providers`,
      or runtime via the admin settings `oauth2.providers` key)
    * authorization URL generation with PKCE + state (via Assent)
    * callback code exchange (via Assent)
    * `_external_auths` linking table (provider + providerId → user record)
    * record find-or-create on OAuth2 sign-in, mirroring PocketBase

  ## Supported providers

  Any [Assent](https://hexdocs.pm/assent) strategy module, e.g.:

  ```elixir
  config :lazypock, :oauth2_providers, [
    google: [
      client_id: "...",
      client_secret: "...",
      redirect_uri: "http://localhost:4000/api/oauth2-redirect"
    ],
    github: [
      client_id: "...",
      client_secret: "...",
      redirect_uri: "http://localhost:4000/api/oauth2-redirect"
    ]
  ]
  ```

  For generic providers, pass `strategy` (an Assent strategy module, default
  `Assent.Strategy.OAuth2`) plus `base_url`, `authorize_url`, `token_url`,
  `user_url` and any `authorization_params`.
  """

  alias Lazypock.Schemas.GenericRecord

  @system_external_auths "_external_auths"
  @session_table :lazypock_oauth2_sessions
  # 10 minutes
  @session_ttl_ms 10 * 60 * 1000

  @doc """
  Ensure the OAuth2 session ETS table exists (called at boot).
  """
  def ensure_session_table! do
    case :ets.whereis(@session_table) do
      :undefined ->
        :ets.new(@session_table, [:named_table, :set, :public, read_concurrency: true])
        :ok

      _pid ->
        :ok
    end
  end

  @doc """
  Store an OAuth2 session (state → {provider, collection, code_verifier}).

  Returns the state key.
  """
  @spec store_session(String.t(), String.t(), String.t(), String.t()) :: String.t()
  def store_session(provider, collection, code_verifier, state) do
    :ets.insert(
      @session_table,
      {state, provider, collection, code_verifier, System.system_time(:millisecond)}
    )

    state
  end

  @doc """
  Look up and consume an OAuth2 session by state.

  Returns `{:ok, provider, collection, code_verifier}` or `{:error, :expired}` /
  `{:error, :not_found}`.
  """
  @spec take_session(String.t()) ::
          {:ok, String.t(), String.t(), String.t()} | {:error, atom()}
  def take_session(state) do
    case :ets.take(@session_table, state) do
      [{^state, provider, collection, code_verifier, ts}] ->
        now = System.system_time(:millisecond)

        if now - ts <= @session_ttl_ms do
          {:ok, provider, collection, code_verifier}
        else
          {:error, :expired}
        end

      [] ->
        {:error, :not_found}
    end
  end

  @doc """
  Ensure the `_external_auths` system table exists (called at boot).

  Mirrors PocketBase's `_externalAuths` collection:
    * collection  — auth collection name (e.g. "users")
    * provider    — provider name (e.g. "google")
    * providerId  — the user's id at the provider
    * user        — the auth record id
    * created / updated
  """
  def ensure_external_auths_table! do
    Ecto.Adapters.SQL.query!(
      Lazypock.Repo,
      """
      CREATE TABLE IF NOT EXISTS _external_auths (
        id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        collection  TEXT NOT NULL,
        provider    TEXT NOT NULL,
        provider_id TEXT NOT NULL,
        user_id     UUID NOT NULL,
        created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
        updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
        UNIQUE (collection, provider, provider_id)
      )
      """,
      []
    )
  end

  @doc "The system `_external_auths` table name."
  def external_auths_table, do: @system_external_auths

  @doc """
  Returns the provider config as a keyword list of `{name, opts}`.

  Runtime settings (admin UI) take precedence over static config.
  """
  @spec providers() :: [{String.t(), keyword()}]
  def providers do
    runtime = Lazypock.Settings.get("oauth2", %{}) |> Map.get("providers", [])

    providers =
      if is_list(runtime) and runtime != [] do
        runtime
      else
        Application.get_env(:lazypock, :oauth2_providers, [])
        |> Enum.map(fn {name, opts} ->
          %{"name" => to_string(name), "config" => opts_to_map(opts)}
        end)
      end

    providers
    |> Enum.filter(&(&1["config"] != %{}))

    providers
    |> Enum.filter(&(&1["config"] != %{}))
    |> Enum.map(fn %{"name" => name, "config" => cfg} ->
      {to_string(name), map_to_keyword(cfg)}
    end)
  end

  # Normalize config maps (string keys) into an atom-keyed keyword list so
  # callers can use `cfg[:client_id]` (PB-style).
  defp map_to_keyword(map) when is_map(map) do
    Enum.map(map, fn {k, v} ->
      key = if is_atom(k), do: k, else: String.to_atom(k)
      {key, v}
    end)
  end

  @doc "Returns the config for a single provider, or `nil`."
  @spec provider_config(String.t()) :: keyword() | nil
  def provider_config(name) do
    Enum.find_value(providers(), fn {provider_name, cfg} ->
      if provider_name == name, do: cfg
    end)
  end

  @doc """
  Returns the Assent strategy module for a provider config.

  Defaults to `Assent.Strategy.OAuth2` (generic) unless a `:strategy`
  key is present.
  """
  def strategy_for(cfg) when is_list(cfg) do
    strategy =
      Keyword.get(cfg, :strategy) ||
        case List.keyfind(cfg, "strategy", 0) do
          {_, v} -> v
          nil -> nil
        end

    case strategy do
      nil ->
        Assent.Strategy.OAuth2

      mod when is_atom(mod) ->
        mod

      # Runtime (settings) configs store the module as a string in JSONB
      mod when is_binary(mod) ->
        String.to_existing_atom(mod)
    end
  end

  @doc """
  Builds the authorization URL for a provider (PKCE + state).

  Returns `{:ok, %{url: url, session_params: session_params}}` where
  `session_params` contains the PKCE `code_verifier` + `state` that must
  be passed back at callback time.
  """
  @spec authorize_url(String.t()) ::
          {:ok, %{url: String.t(), session_params: map()}} | {:error, term()}
  def authorize_url(provider_name) do
    with cfg when is_list(cfg) <- provider_config(provider_name) do
      strategy = strategy_for(cfg)
      config = assented_config(cfg)

      case strategy.authorize_url(config) do
        {:ok, %{url: url, session_params: session_params}} ->
          {:ok, %{url: url, session_params: session_params}}

        {:error, error} ->
          {:error, error}
      end
    else
      nil -> {:error, :unknown_provider}
    end
  end

  @doc """
  Exchanges the OAuth2 authorization `code` for tokens + user info.

  Pass back the `session_params` from `authorize_url/1` (contains the PKCE
  `code_verifier` + `state`).

  Set `state: false` in `session_params` to skip state verification (used by
  the direct code-exchange endpoint where the client provides `codeVerifier`
  but not the state).

  Returns `{:ok, %{user: user, token: token}}` (Assent shape) or `{:error, ...}`.
  """
  @spec callback(String.t(), map(), map()) ::
          {:ok, %{user: map(), token: map()}} | {:error, term()}
  def callback(provider_name, params, session_params) do
    with cfg when is_list(cfg) <- provider_config(provider_name) do
      strategy = strategy_for(cfg)

      config =
        assented_config(cfg)
        |> Keyword.put(:session_params, session_params)
        |> maybe_disable_state(session_params)

      strategy.callback(config, params)
    else
      nil -> {:error, :unknown_provider}
    end
  end

  defp maybe_disable_state(config, %{state: false}), do: Keyword.put(config, :state, false)
  defp maybe_disable_state(config, _session_params), do: config

  @doc """
  Find an external auth link, or create it if missing.

  Returns `{:ok, external_auth}` where `external_auth` is a map with the
  `_external_auths` row.
  """
  @spec find_or_create_external_auth(String.t(), String.t(), String.t(), String.t()) ::
          {:ok, map()} | {:error, term()}
  def find_or_create_external_auth(collection, provider, provider_id, user_id) do
    case find_external_auth(collection, provider, provider_id) do
      {:ok, link} ->
        {:ok, link}

      {:error, :not_found} ->
        # user_id targets a UUID column — convert to Postgrex binary form
        user_id_bin = maybe_uuid_to_bin(user_id)

        case GenericRecord.insert(@system_external_auths, %{
               "collection" => collection,
               "provider" => provider,
               "provider_id" => provider_id,
               "user_id" => user_id_bin
             }) do
          {:ok, link} -> {:ok, link}
          {:error, reason} -> {:error, reason}
        end
    end
  end

  defp maybe_uuid_to_bin(id) when is_binary(id) do
    case Ecto.UUID.dump(id) do
      {:ok, bin} -> bin
      :error -> id
    end
  end

  defp maybe_uuid_to_bin(id), do: id

  @doc """
  Find an external auth link by collection + provider + providerId.

  Returns `{:ok, link}` or `{:error, :not_found}`.
  """
  @spec find_external_auth(String.t(), String.t(), String.t()) ::
          {:ok, map()} | {:error, :not_found}
  def find_external_auth(collection, provider, provider_id) do
    sql = """
    SELECT * FROM #{Lazypock.Schema.TypeMapper.quote_ident(@system_external_auths)}
    WHERE collection = $1 AND provider = $2 AND provider_id = $3
    LIMIT 1
    """

    case Ecto.Adapters.SQL.query(Lazypock.Repo, sql, [collection, provider, provider_id]) do
      {:ok, %{rows: [row], columns: cols}} -> {:ok, row_to_map(cols, row)}
      {:ok, _} -> {:error, :not_found}
      {:error, _} -> {:error, :not_found}
    end
  end

  defp row_to_map(cols, row) do
    cols
    |> Enum.zip(row)
    |> Enum.map(fn {col, val} -> {col, coerce_value(col, val)} end)
    |> Map.new()
  end

  defp coerce_value(col, v) when is_binary(v) and byte_size(v) == 16 do
    if uuid_column?(col), do: Ecto.UUID.cast!(v), else: v
  end

  defp coerce_value(_col, %Decimal{} = d), do: Decimal.to_float(d)
  defp coerce_value(_col, %Date{} = d), do: Date.to_iso8601(d)
  defp coerce_value(_col, %NaiveDateTime{} = dt), do: NaiveDateTime.to_iso8601(dt)
  defp coerce_value(_col, %DateTime{} = dt), do: DateTime.to_iso8601(dt)
  defp coerce_value(_col, v), do: v

  defp uuid_column?(col) do
    col == "id" or String.ends_with?(col, "_id") or String.ends_with?(col, "_ref")
  end

  @doc """
  Load the auth user record linked to an external auth, if any.

  Returns `{:ok, record}` or `{:error, :not_found}`.
  """
  @spec linked_user(String.t(), map()) :: {:ok, map()} | {:error, :not_found}
  def linked_user(collection_name, %{"user_id" => user_id}) do
    case GenericRecord.get(collection_name, user_id) do
      nil -> {:error, :not_found}
      record -> {:ok, record}
    end
  end

  # ── Private helpers ───────────────────────────────────

  @doc """
  Find the auth record for an OAuth2 user, creating it if needed.

  Mirrors PocketBase's `authWithOAuth2` record handling:

    * If an `_external_auths` link exists for `(collection, provider, provider_id)`,
      return the linked record (`is_new = false`).
    * Otherwise create a new record from the provider's user info (`is_new = true`),
      then create the external auth link.

  `create_data` (optional) is merged into the new record on sign-up.

  Returns `{:ok, %{record: record, is_new: boolean, external_auth: link}}`
  or `{:error, reason}`.
  """
  @spec find_or_create_record(String.t(), map(), String.t(), map(), map(), map()) ::
          {:ok, %{record: map(), is_new: boolean(), external_auth: map()}} | {:error, term()}
  def find_or_create_record(
        collection_name,
        collection,
        provider,
        provider_id,
        oauth2_user,
        create_data \\ %{}
      ) do
    email_field = find_email_field(collection)
    password_field = find_password_field(collection)

    case find_external_auth(collection_name, provider, provider_id) do
      {:ok, link} ->
        case linked_user(collection_name, link) do
          {:ok, record} ->
            {:ok, %{record: record, is_new: false, external_auth: link}}

          {:error, :not_found} ->
            create_record(
              collection_name,
              collection,
              provider,
              provider_id,
              oauth2_user,
              create_data,
              email_field,
              password_field
            )
        end

      {:error, :not_found} ->
        create_record(
          collection_name,
          collection,
          provider,
          provider_id,
          oauth2_user,
          create_data,
          email_field,
          password_field
        )
    end
  end

  defp create_record(
         collection_name,
         collection,
         provider,
         provider_id,
         oauth2_user,
         create_data,
         email_field,
         password_field
       ) do
    field_names = Enum.map(collection.fields || [], & &1.name)

    attrs =
      %{}
      |> Map.put(email_field, oauth2_user["email"])
      |> Map.put(password_field, random_password())
      |> maybe_put(oauth2_user["name"], :name)
      |> maybe_put(oauth2_user["picture"], :avatar)
      |> Map.merge(create_data || %{})
      |> Map.take(field_names)

    case GenericRecord.insert(collection_name, attrs) do
      {:ok, record} ->
        case find_or_create_external_auth(collection_name, provider, provider_id, record["id"]) do
          {:ok, link} -> {:ok, %{record: record, is_new: true, external_auth: link}}
          {:error, reason} -> {:error, reason}
        end

      {:error, reason} ->
        {:error, reason}
    end
  end

  defp maybe_put(map, nil, _key), do: map
  defp maybe_put(map, value, key), do: Map.put(map, to_string(key), value)

  defp random_password, do: :crypto.strong_rand_bytes(24) |> Base.url_encode64(padding: false)

  defp find_email_field(collection) do
    email_fields =
      (collection.fields || [])
      |> Enum.filter(fn f -> f.type == "email" end)
      |> Enum.map(fn f -> f.name end)

    case email_fields do
      [name | _] -> name
      [] -> "email"
    end
  end

  defp find_password_field(collection) do
    password_fields =
      (collection.fields || [])
      |> Enum.filter(fn f -> f.type == "password" end)
      |> Enum.map(fn f -> f.name end)

    case password_fields do
      [name | _] -> name
      [] -> "password_hash"
    end
  end

  defp opts_to_map(opts) when is_list(opts) do
    Enum.reduce(opts, %{}, fn {k, v}, acc -> Map.put(acc, to_string(k), v) end)
  end

  defp opts_to_map(%{} = map), do: map

  # Build the Assent config keyword list from our provider config.
  # Assent expects string keys? No — it expects atom keys for client_id etc.
  defp assented_config(cfg) when is_list(cfg) do
    Enum.map(cfg, fn
      {:client_id, v} -> {:client_id, v}
      {:client_secret, v} -> {:client_secret, v}
      {:redirect_uri, v} -> {:redirect_uri, v}
      {:authorize_url, v} -> {:authorize_url, v}
      {:token_url, v} -> {:token_url, v}
      {:user_url, v} -> {:user_url, v}
      {:base_url, v} -> {:base_url, v}
      {:strategy, v} -> {:strategy, v}
      {k, v} when is_atom(k) -> {k, v}
      {k, v} -> {String.to_atom(k), v}
    end)
    |> Keyword.put_new(:code_verifier, true)
  end
end
