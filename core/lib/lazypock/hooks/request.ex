defmodule Lazypock.Hooks.Request do
  @moduledoc """
  PocketBase request hooks — fired only when the corresponding API endpoint is
  accessed. They carry the full request context (unlike the lower-level
  Record/Collection model hooks).

  Full parity with the PocketBase docs:

  ### Record CRUD request hooks
    * `onRecordsListRequest` — `e.collection`, `e.records`, `e.result`
    * `onRecordViewRequest` — `e.collection`, `e.record`
    * `onRecordCreateRequest` — `e.collection`, `e.record`
    * `onRecordUpdateRequest` — `e.collection`, `e.record`
    * `onRecordDeleteRequest` — `e.collection`, `e.record`

  ### Record auth request hooks
    * `onRecordAuthRequest` — `e.record`, `e.token`, `e.meta`, `e.authMethod`
    * `onRecordAuthRefreshRequest` — `e.collection`, `e.record`
    * `onRecordAuthWithPasswordRequest` — `e.record` (nullable), `e.identity`,
      `e.identityField`, `e.password`
    * `onRecordAuthWithOAuth2Request` — `e.providerName`, `e.providerClient`,
      `e.record` (nullable), `e.oauth2User`, `e.createData`, `e.isNewRecord`
    * `onRecordRequestPasswordResetRequest` — `e.collection`, `e.record`
    * `onRecordConfirmPasswordResetRequest` — `e.collection`, `e.record`
    * `onRecordRequestVerificationRequest` — `e.collection`, `e.record`
    * `onRecordConfirmVerificationRequest` — `e.collection`, `e.record`
    * `onRecordRequestEmailChangeRequest` — `e.collection`, `e.record`, `e.newEmail`
    * `onRecordConfirmEmailChangeRequest` — `e.collection`, `e.record`, `e.newEmail`
    * `onRecordRequestOTPRequest` — `e.collection`, `e.record` (nullable), `e.password`
    * `onRecordAuthWithOTPRequest` — `e.collection`, `e.record`, `e.otp`

  ### Batch / File / Collection / Settings request hooks
    * `onBatchRequest` — `e.batch`
    * `onFileDownloadRequest` — `e.collection`, `e.record`, `e.fileField`,
      `e.servedPath`, `e.servedName`
    * `onFileTokenRequest` — `e.record`, `e.token`
    * `onCollectionsListRequest` — `e.collections`, `e.result`
    * `onCollectionViewRequest` — `e.collection`
    * `onCollectionCreateRequest` — `e.collection`
    * `onCollectionUpdateRequest` — `e.collection`
    * `onCollectionDeleteRequest` — `e.collection`
    * `onCollectionsImportRequest` — `e.collectionsData`, `e.deleteMissing`
    * `onSettingsListRequest` — `e.settings`
    * `onSettingsUpdateRequest` — `e.oldSettings`, `e.newSettings`

  Every handler receives `e.app`, `e.request` (the raw Phoenix conn),
  `e.request_info` and `e.collection_name` (when applicable), plus the
  documented per-hook fields — and all RequestEvent fields (`e.request`,
  `e.response`, `e.auth`, ...).
  """

  alias Lazypock.Hooks.Registry

  # ── Registration helpers ────────────────────────────────

  @doc "Registers an `on_records_list_request` handler."
  def on_records_list_request(fun, opts \\ []),
    do: register(:on_records_list_request, fun, opts)

  @doc "Registers an `on_record_view_request` handler."
  def on_record_view_request(fun, opts \\ []), do: register(:on_record_view_request, fun, opts)

  @doc "Registers an `on_record_create_request` handler."
  def on_record_create_request(fun, opts \\ []),
    do: register(:on_record_create_request, fun, opts)

  @doc "Registers an `on_record_update_request` handler."
  def on_record_update_request(fun, opts \\ []),
    do: register(:on_record_update_request, fun, opts)

  @doc "Registers an `on_record_delete_request` handler."
  def on_record_delete_request(fun, opts \\ []),
    do: register(:on_record_delete_request, fun, opts)

  @doc "Registers an `on_record_auth_request` handler."
  def on_record_auth_request(fun, opts \\ []), do: register(:on_record_auth_request, fun, opts)

  @doc "Registers an `on_record_auth_refresh_request` handler."
  def on_record_auth_refresh_request(fun, opts \\ []),
    do: register(:on_record_auth_refresh_request, fun, opts)

  @doc "Registers an `on_record_auth_with_password_request` handler."
  def on_record_auth_with_password_request(fun, opts \\ []),
    do: register(:on_record_auth_with_password_request, fun, opts)

  @doc "Registers an `on_record_auth_with_oauth2_request` handler."
  def on_record_auth_with_oauth2_request(fun, opts \\ []),
    do: register(:on_record_auth_with_oauth2_request, fun, opts)

  @doc "Registers an `on_record_request_password_reset_request` handler."
  def on_record_request_password_reset_request(fun, opts \\ []),
    do: register(:on_record_request_password_reset_request, fun, opts)

  @doc "Registers an `on_record_confirm_password_reset_request` handler."
  def on_record_confirm_password_reset_request(fun, opts \\ []),
    do: register(:on_record_confirm_password_reset_request, fun, opts)

  @doc "Registers an `on_record_request_verification_request` handler."
  def on_record_request_verification_request(fun, opts \\ []),
    do: register(:on_record_request_verification_request, fun, opts)

  @doc "Registers an `on_record_confirm_verification_request` handler."
  def on_record_confirm_verification_request(fun, opts \\ []),
    do: register(:on_record_confirm_verification_request, fun, opts)

  @doc "Registers an `on_record_request_email_change_request` handler."
  def on_record_request_email_change_request(fun, opts \\ []),
    do: register(:on_record_request_email_change_request, fun, opts)

  @doc "Registers an `on_record_confirm_email_change_request` handler."
  def on_record_confirm_email_change_request(fun, opts \\ []),
    do: register(:on_record_confirm_email_change_request, fun, opts)

  @doc "Registers an `on_record_request_otp_request` handler."
  def on_record_request_otp_request(fun, opts \\ []),
    do: register(:on_record_request_otp_request, fun, opts)

  @doc "Registers an `on_record_auth_with_otp_request` handler."
  def on_record_auth_with_otp_request(fun, opts \\ []),
    do: register(:on_record_auth_with_otp_request, fun, opts)

  @doc "Registers an `on_batch_request` handler."
  def on_batch_request(fun, opts \\ []), do: register(:on_batch_request, fun, opts)

  @doc "Registers an `on_file_download_request` handler."
  def on_file_download_request(fun, opts \\ []),
    do: register(:on_file_download_request, fun, opts)

  @doc "Registers an `on_file_token_request` handler."
  def on_file_token_request(fun, opts \\ []), do: register(:on_file_token_request, fun, opts)

  @doc "Registers an `on_collections_list_request` handler."
  def on_collections_list_request(fun, opts \\ []),
    do: register(:on_collections_list_request, fun, opts)

  @doc "Registers an `on_collection_view_request` handler."
  def on_collection_view_request(fun, opts \\ []),
    do: register(:on_collection_view_request, fun, opts)

  @doc "Registers an `on_collection_create_request` handler."
  def on_collection_create_request(fun, opts \\ []),
    do: register(:on_collection_create_request, fun, opts)

  @doc "Registers an `on_collection_update_request` handler."
  def on_collection_update_request(fun, opts \\ []),
    do: register(:on_collection_update_request, fun, opts)

  @doc "Registers an `on_collection_delete_request` handler."
  def on_collection_delete_request(fun, opts \\ []),
    do: register(:on_collection_delete_request, fun, opts)

  @doc "Registers an `on_collections_import_request` handler."
  def on_collections_import_request(fun, opts \\ []),
    do: register(:on_collections_import_request, fun, opts)

  @doc "Registers an `on_settings_list_request` handler."
  def on_settings_list_request(fun, opts \\ []),
    do: register(:on_settings_list_request, fun, opts)

  @doc "Registers an `on_settings_update_request` handler."
  def on_settings_update_request(fun, opts \\ []),
    do: register(:on_settings_update_request, fun, opts)

  defp register(event, fun, opts) when is_function(fun, 1) do
    Registry.register(event, {:fun, fun}, opts)
    :ok
  end

  defp register(_event, _fun, _opts), do: {:error, :invalid_handler}

  # ── Trigger points (called by the framework) ────────────

  @doc "Fires `on_records_list_request`."
  def trigger_records_list(conn, collection_name, collection, records, result) do
    trigger(:on_records_list_request, conn, collection_name, %{
      collection: collection,
      records: records,
      result: result
    })
  end

  @doc "Fires `on_record_view_request`."
  def trigger_record_view(conn, collection_name, collection, record) do
    trigger(:on_record_view_request, conn, collection_name, %{
      collection: collection,
      record: record
    })
  end

  @doc "Fires `on_record_create_request`."
  def trigger_record_create(conn, collection_name, collection, record) do
    trigger(:on_record_create_request, conn, collection_name, %{
      collection: collection,
      record: record
    })
  end

  @doc "Fires `on_record_update_request`."
  def trigger_record_update(conn, collection_name, collection, record) do
    trigger(:on_record_update_request, conn, collection_name, %{
      collection: collection,
      record: record
    })
  end

  @doc "Fires `on_record_delete_request`."
  def trigger_record_delete(conn, collection_name, collection, record) do
    trigger(:on_record_delete_request, conn, collection_name, %{
      collection: collection,
      record: record
    })
  end

  @doc "Fires `on_record_auth_request`."
  def trigger_record_auth(conn, collection_name, record, token, meta, auth_method) do
    trigger(:on_record_auth_request, conn, collection_name, %{
      record: record,
      token: token,
      meta: meta,
      authMethod: auth_method
    })
  end

  @doc "Fires `on_record_auth_refresh_request`."
  def trigger_record_auth_refresh(conn, collection_name, collection, record) do
    trigger(:on_record_auth_refresh_request, conn, collection_name, %{
      collection: collection,
      record: record
    })
  end

  @doc "Fires `on_record_auth_with_password_request`."
  def trigger_record_auth_with_password(
        conn,
        collection_name,
        collection,
        record,
        identity,
        identity_field,
        password
      ) do
    trigger(:on_record_auth_with_password_request, conn, collection_name, %{
      collection: collection,
      record: record,
      identity: identity,
      identityField: identity_field,
      password: password
    })
  end

  @doc "Fires `on_record_auth_with_oauth2_request`."
  def trigger_record_auth_with_oauth2(
        conn,
        collection_name,
        collection,
        provider_name,
        provider_client,
        record,
        oauth2_user,
        create_data,
        is_new_record
      ) do
    trigger(:on_record_auth_with_oauth2_request, conn, collection_name, %{
      collection: collection,
      providerName: provider_name,
      providerClient: provider_client,
      record: record,
      oauth2User: oauth2_user,
      createData: create_data,
      isNewRecord: is_new_record
    })
  end

  @doc "Fires `on_record_request_password_reset_request`."
  def trigger_record_request_password_reset(conn, collection_name, collection, record) do
    trigger(:on_record_request_password_reset_request, conn, collection_name, %{
      collection: collection,
      record: record
    })
  end

  @doc "Fires `on_record_confirm_password_reset_request`."
  def trigger_record_confirm_password_reset(conn, collection_name, collection, record) do
    trigger(:on_record_confirm_password_reset_request, conn, collection_name, %{
      collection: collection,
      record: record
    })
  end

  @doc "Fires `on_record_request_verification_request`."
  def trigger_record_request_verification(conn, collection_name, collection, record) do
    trigger(:on_record_request_verification_request, conn, collection_name, %{
      collection: collection,
      record: record
    })
  end

  @doc "Fires `on_record_confirm_verification_request`."
  def trigger_record_confirm_verification(conn, collection_name, collection, record) do
    trigger(:on_record_confirm_verification_request, conn, collection_name, %{
      collection: collection,
      record: record
    })
  end

  @doc "Fires `on_record_request_email_change_request`."
  def trigger_record_request_email_change(conn, collection_name, collection, record, new_email) do
    trigger(:on_record_request_email_change_request, conn, collection_name, %{
      collection: collection,
      record: record,
      newEmail: new_email
    })
  end

  @doc "Fires `on_record_confirm_email_change_request`."
  def trigger_record_confirm_email_change(conn, collection_name, collection, record, new_email) do
    trigger(:on_record_confirm_email_change_request, conn, collection_name, %{
      collection: collection,
      record: record,
      newEmail: new_email
    })
  end

  @doc "Fires `on_record_request_otp_request`."
  def trigger_record_request_otp(conn, collection_name, collection, record, password) do
    trigger(:on_record_request_otp_request, conn, collection_name, %{
      collection: collection,
      record: record,
      password: password
    })
  end

  @doc "Fires `on_record_auth_with_otp_request`."
  def trigger_record_auth_with_otp(conn, collection_name, collection, record, otp) do
    trigger(:on_record_auth_with_otp_request, conn, collection_name, %{
      collection: collection,
      record: record,
      otp: otp
    })
  end

  @doc "Fires `on_batch_request`."
  def trigger_batch(conn, collection_name, batch) do
    trigger(:on_batch_request, conn, collection_name, %{batch: batch})
  end

  @doc "Fires `on_file_download_request`."
  def trigger_file_download(conn, collection_name, nil) do
    # Convenience overload for the generic /api/files/:id store (no collection
    # context) — fires the hook with only the request info.
    trigger(:on_file_download_request, conn, collection_name, %{})
  end

  def trigger_file_download(
        conn,
        collection_name,
        collection,
        record,
        file_field,
        served_path,
        served_name
      ) do
    trigger(:on_file_download_request, conn, collection_name, %{
      collection: collection,
      record: record,
      fileField: file_field,
      servedPath: served_path,
      servedName: served_name
    })
  end

  @doc "Fires `on_file_token_request`."
  def trigger_file_token(conn, collection_name, record, token) do
    trigger(:on_file_token_request, conn, collection_name, %{record: record, token: token})
  end

  @doc "Fires `on_collections_list_request`."
  def trigger_collections_list(conn, collections, result) do
    trigger(:on_collections_list_request, conn, nil, %{collections: collections, result: result})
  end

  @doc "Fires `on_collection_view_request`."
  def trigger_collection_view(conn, collection) do
    trigger(:on_collection_view_request, conn, collection.name, %{collection: collection})
  end

  @doc "Fires `on_collection_create_request`."
  def trigger_collection_create(conn, collection) do
    trigger(:on_collection_create_request, conn, collection.name, %{collection: collection})
  end

  @doc "Fires `on_collection_update_request`."
  def trigger_collection_update(conn, collection) do
    trigger(:on_collection_update_request, conn, collection.name, %{collection: collection})
  end

  @doc "Fires `on_collection_delete_request`."
  def trigger_collection_delete(conn, collection) do
    trigger(:on_collection_delete_request, conn, collection.name, %{collection: collection})
  end

  @doc "Fires `on_collections_import_request`."
  def trigger_collections_import(conn, collections_data, delete_missing) do
    trigger(:on_collections_import_request, conn, nil, %{
      collectionsData: collections_data,
      deleteMissing: delete_missing
    })
  end

  @doc "Fires `on_settings_list_request`."
  def trigger_settings_list(conn, settings) do
    trigger(:on_settings_list_request, conn, nil, %{settings: settings})
  end

  @doc "Fires `on_settings_update_request`."
  def trigger_settings_update(conn, old_settings, new_settings) do
    trigger(:on_settings_update_request, conn, nil, %{
      oldSettings: old_settings,
      newSettings: new_settings
    })
  end

  # ── Shared trigger machinery ────────────────────────────

  defp trigger(event, conn, collection_name, data) do
    request_info = build_request_info(conn)

    data =
      data
      |> Map.put(:app, Lazypock.app())
      |> Map.put(:request, conn)
      |> Map.put(:request_info, request_info)

    Registry.dispatch(event, data, collection_name)
  end

  defp build_request_info(conn) do
    %{
      auth: conn.assigns[:current_user] || conn.assigns[:current_superuser],
      method: conn.method,
      path: conn.request_path,
      query: conn.query_params,
      headers: conn.req_headers |> Enum.into(%{}),
      body: conn.body_params
    }
  end
end
