defmodule LazypockWeb.Router do
  use LazypockWeb, :router

  # Serve the Svelte SPA admin UI (static files)
  # In production: priv/static/admin/ is built by the Svelte project
  # In dev: the Svelte dev server proxies to avoid CORS issues
  pipeline :spa do
    plug(:accepts, ["html", "json"])
    plug(:fetch_session)
  end

  pipeline :api do
    plug(:accepts, ["json"])
    plug(LazypockWeb.Plugs.RequestLogger)
  end

  pipeline :auth do
    plug(:accepts, ["json"])
    plug(LazypockWeb.Plugs.RequestLogger)
    plug(Lazypock.Auth.Plug)
  end

  # Admin SPA — catch-all for /_/*
  scope "/_", LazypockWeb do
    pipe_through(:spa)

    get("/*path", AdminSpaController, :index)
  end

  # Health check (no auth)
  scope "/api", LazypockWeb do
    pipe_through(:api)

    get("/health", HealthController, :index)

    # Superuser auth (no auth required)
    get("/superusers/check", SuperUserController, :check)
    post("/superusers/setup", SuperUserController, :setup)
    post("/superusers/login", SuperUserController, :login)
  end

  # Authenticated API routes — token is verified but NOT hard-blocked here.
  # Access control is handled by each controller/enforcer.
  scope "/api", LazypockWeb do
    pipe_through(:auth)

    get("/superusers/me", SuperUserController, :me)

    # Collection management (DDL operations — enforcer checks superuser/rules)
    get("/collections", CollectionController, :list)
    post("/collections", CollectionController, :create)
    get("/collections/:id", CollectionController, :show)
    patch("/collections/:id", CollectionController, :update)
    delete("/collections/:id", CollectionController, :delete)

    # File routes — must be BEFORE dynamic :collection routes
    get("/files", FileController, :index)
    post("/files", FileController, :upload)
    get("/files/:id/thumbs/:size", FileController, :show_thumb)
    get("/files/:id/scale/:size", FileController, :show_scaled)
    get("/files/:id", FileController, :show)
    delete("/files/:id", FileController, :delete)

    # Request logs (superuser)
    get("/logs", LogsController, :list)
    get("/logs/stats", LogsController, :stats)
    get("/logs/collections", LogsController, :collections)
    get("/logs/:id", LogsController, :show)
    delete("/logs", LogsController, :delete_logs)

    # App settings (superuser)
    get("/settings", SettingsController, :show)
    patch("/settings", SettingsController, :update)
    put("/settings", SettingsController, :update)

    # App settings — API key management
    get("/settings/api-keys", SettingsController, :list_api_keys)
    post("/settings/api-keys", SettingsController, :generate_api_key)
    delete("/settings/api-keys/:id", SettingsController, :revoke_api_key)
    # Back-compat single-key aliases
    get("/settings/api-key", SettingsController, :get_api_key)
    post("/settings/api-key", SettingsController, :generate_api_key)

    # SQL console — read-only queries (superuser)
    post("/sql/query", SettingsController, :sql_query)

    # Export/Import collections
    get("/export", SettingsController, :export_all)
    post("/import", SettingsController, :import_all)

    # Send test email (superuser)
    post("/settings/test-email", SettingsController, :send_test_email)
  end

  # Auth collection routes — must be BEFORE dynamic :collection routes
  # These use :collection param just like dynamic routes, but with specific path suffixes
  scope "/api", LazypockWeb do
    pipe_through(:api)

    # No auth required (public login/methods)
    post("/:collection/auth-with-password", AuthController, :auth_with_password)
    get("/:collection/auth-methods", AuthController, :auth_methods)

    # Email verification & password reset (public — no auth)
    post("/:collection/request-verification", EmailController, :request_verification)
    post("/:collection/confirm-verification", EmailController, :confirm_verification)
    post("/:collection/request-password-reset", EmailController, :request_password_reset)
    post("/:collection/confirm-password-reset", EmailController, :confirm_password_reset)
  end

  scope "/api", LazypockWeb do
    pipe_through(:auth)

    # Auth required (token refresh)
    post("/:collection/auth-refresh", AuthController, :auth_refresh)
  end

  # Dynamic collection routes — must be LAST
  scope "/api", LazypockWeb do
    pipe_through(:auth)

    get("/:collection", DynamicController, :list)
    get("/:collection/:id", DynamicController, :show)
    post("/:collection", DynamicController, :create)
    patch("/:collection/:id", DynamicController, :update)
    put("/:collection/:id", DynamicController, :update)
    delete("/:collection/:id", DynamicController, :delete)
  end
end
