defmodule Lazypock.Hooks.Hook do
  @moduledoc """
  User-facing macro for writing LazyPock hooks — the PocketBase JS event
  hooks model expressed in Elixir.

  Every handler receives a `Lazypock.Hooks.Event` (`e`) and must call
  `Lazypock.Hooks.Event.next(e)` (or return `{:ok, e}`) to proceed with the
  execution chain. Returning `{:error, reason}` — or raising — stops the
  chain, exactly like PocketBase ("throwing an error or not calling
  `e.next()` inside a handler function stops the hook execution chain").

  ## Example

      # priv/hooks/posts_hooks.ex
      defmodule PostsHooks do
        use Lazypock.Hooks.Hook, collection: "posts"

        # PocketBase: onRecordCreate((e) => { e.record.slug = ...; e.next() })
        def on_record_create(e) do
          slug = e.record["title"] |> to_string() |> String.downcase()
          e = Lazypock.Hooks.Event.put(e, :record, Map.put(e.record, "slug", slug))
          Lazypock.Hooks.Event.next(e)
        end

        # PocketBase: onRecordAfterCreateSuccess((e) => { ...; e.next() })
        def on_record_after_create_success(e) do
          Logger.info("record created: \#{e.record["id"]}")
          Lazypock.Hooks.Event.next(e)
        end

        # PocketBase: onRecordValidate + collection filter
        def on_record_validate(e) do
          if e.record["title"] == "" do
            {:error, "title is required"}
          else
            Lazypock.Hooks.Event.next(e)
          end
        end
      end

  ## Available events

  Define any of the following functions (all receive `e`, all must call
  `Event.next(e)` or return `{:ok, e}` to proceed):

  ### App hooks
    * `on_bootstrap/1`, `on_settings_reload/1`, `on_backup_create/1`,
      `on_backup_restore/1`, `on_terminate/1`, `on_before_serve/1`

  ### Record model hooks
    * `on_record_enrich/1`, `on_record_validate/1`
    * `on_record_create/1`, `on_record_create_execute/1`,
      `on_record_after_create_success/1`, `on_record_after_create_error/1`
    * `on_record_update/1`, `on_record_update_execute/1`,
      `on_record_after_update_success/1`, `on_record_after_update_error/1`
    * `on_record_delete/1`, `on_record_delete_execute/1`,
      `on_record_after_delete_success/1`, `on_record_after_delete_error/1`

  ### Collection model hooks
    * `on_collection_validate/1` and the same
      create/update/delete × execute/after-success/after-error matrix

  ### Base model hooks
    * `on_model_validate/1` and the same create/update/delete matrix

  ### Request hooks (fired only on API endpoint access)
    * `on_records_list_request/1`, `on_record_view_request/1`,
      `on_record_create_request/1`, `on_record_update_request/1`,
      `on_record_delete_request/1`
    * Auth: `on_record_auth_request/1`, `on_record_auth_refresh_request/1`,
      `on_record_auth_with_password_request/1`,
      `on_record_auth_with_oauth2_request/1`,
      `on_record_request_password_reset_request/1`,
      `on_record_confirm_password_reset_request/1`,
      `on_record_request_verification_request/1`,
      `on_record_confirm_verification_request/1`,
      `on_record_request_email_change_request/1`,
      `on_record_confirm_email_change_request/1`,
      `on_record_request_otp_request/1`, `on_record_auth_with_otp_request/1`
    * Batch/File/Collection/Settings: `on_batch_request/1`,
      `on_file_download_request/1`, `on_file_token_request/1`,
      `on_collections_list_request/1`, `on_collection_view_request/1`,
      `on_collection_create_request/1`, `on_collection_update_request/1`,
      `on_collection_delete_request/1`, `on_collections_import_request/1`,
      `on_settings_list_request/1`, `on_settings_update_request/1`

  ### Mailer hooks
    * `on_mailer_send/1`, `on_mailer_record_auth_alert_send/1`,
      `on_mailer_record_password_reset_send/1`,
      `on_mailer_record_verification_send/1`,
      `on_mailer_record_email_change_send/1`, `on_mailer_record_otp_send/1`

  ### Realtime hooks
    * `on_realtime_connect_request/1`, `on_realtime_subscribe_request/1`,
      `on_realtime_message_send/1`

  ## Options

    * `:collection` — if given, the hook module fires only for that
      collection (PocketBase's trailing `"users"`, `"articles"` args).
      Without it, the module's record hooks fire for every collection.

  ## Custom API routes

  Register routes via `on_before_serve` + `Lazypock.Hooks.Router.add/4`
  (see `Lazypock.Hooks.Router`):

      def on_before_serve(e) do
        routes = Lazypock.Hooks.Router.add(e, "GET", "/hello/{name}", fn ctx ->
          Lazypock.Hooks.Router.json(ctx, 200, %{"message" => "Hello \#{ctx.params["name"]}"})
        end)
        e = Lazypock.Hooks.Event.put(e, :routes, routes)
        Lazypock.Hooks.Event.next(e)
      end
  """

  defmacro __using__(opts) do
    quote bind_quoted: [opts: opts] do
      import Lazypock.Hooks.Hook, only: []

      collection = Keyword.get(opts, :collection)
      @hook_collection collection

      @doc false
      def __collection__, do: @hook_collection

      @doc false
      def __hook_registrations__ do
        events = [
          :on_bootstrap,
          :on_settings_reload,
          :on_backup_create,
          :on_backup_restore,
          :on_terminate,
          :on_before_serve,
          :on_record_enrich,
          :on_record_validate,
          :on_record_create,
          :on_record_create_execute,
          :on_record_after_create_success,
          :on_record_after_create_error,
          :on_record_update,
          :on_record_update_execute,
          :on_record_after_update_success,
          :on_record_after_update_error,
          :on_record_delete,
          :on_record_delete_execute,
          :on_record_after_delete_success,
          :on_record_after_delete_error,
          :on_collection_validate,
          :on_collection_create,
          :on_collection_create_execute,
          :on_collection_after_create_success,
          :on_collection_after_create_error,
          :on_collection_update,
          :on_collection_update_execute,
          :on_collection_after_update_success,
          :on_collection_after_update_error,
          :on_collection_delete,
          :on_collection_delete_execute,
          :on_collection_after_delete_success,
          :on_collection_after_delete_error,
          :on_model_validate,
          :on_model_create,
          :on_model_create_execute,
          :on_model_after_create_success,
          :on_model_after_create_error,
          :on_model_update,
          :on_model_update_execute,
          :on_model_after_update_success,
          :on_model_after_update_error,
          :on_model_delete,
          :on_model_delete_execute,
          :on_model_after_delete_success,
          :on_model_after_delete_error,
          :on_records_list_request,
          :on_record_view_request,
          :on_record_create_request,
          :on_record_update_request,
          :on_record_delete_request,
          :on_record_auth_request,
          :on_record_auth_refresh_request,
          :on_record_auth_with_password_request,
          :on_record_auth_with_oauth2_request,
          :on_record_request_password_reset_request,
          :on_record_confirm_password_reset_request,
          :on_record_request_verification_request,
          :on_record_confirm_verification_request,
          :on_record_request_email_change_request,
          :on_record_confirm_email_change_request,
          :on_record_request_otp_request,
          :on_record_auth_with_otp_request,
          :on_batch_request,
          :on_file_download_request,
          :on_file_token_request,
          :on_collections_list_request,
          :on_collection_view_request,
          :on_collection_create_request,
          :on_collection_update_request,
          :on_collection_delete_request,
          :on_collections_import_request,
          :on_settings_list_request,
          :on_settings_update_request,
          :on_mailer_send,
          :on_mailer_record_auth_alert_send,
          :on_mailer_record_password_reset_send,
          :on_mailer_record_verification_send,
          :on_mailer_record_email_change_send,
          :on_mailer_record_otp_send,
          :on_realtime_connect_request,
          :on_realtime_subscribe_request,
          :on_realtime_message_send
        ]

        collection_filter = if is_nil(@hook_collection), do: nil, else: [@hook_collection]

        events
        |> Enum.filter(&function_exported?(__MODULE__, &1, 1))
        |> Enum.map(fn event ->
          {event, {__MODULE__, event}, [collections: collection_filter]}
        end)
      end
    end
  end
end
