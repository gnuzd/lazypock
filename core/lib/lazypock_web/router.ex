defmodule LazypockWeb.Router do
  use LazypockWeb, :router

  pipeline :browser do
    plug(:accepts, ["html"])
    plug(:fetch_session)
    plug(:fetch_live_flash)
    plug(:put_root_layout, html: {LazypockWeb.Layouts, :root})
    plug(:protect_from_forgery)
    plug(:put_secure_browser_headers)
  end

  pipeline :api do
    plug(:accepts, ["json"])
  end

  pipeline :auth do
    plug(:accepts, ["json"])
    plug(Lazypock.Auth.Plug)
  end

  scope "/", LazypockWeb do
    pipe_through(:browser)

    get("/", PageController, :home)
  end

  # API routes
  scope "/api", LazypockWeb do
    pipe_through(:api)

    # Health check
    get("/health", HealthController, :index)

    # Superuser auth (no auth required)
    get("/superusers/check", SuperUserController, :check)
    post("/superusers/setup", SuperUserController, :setup)
    post("/superusers/login", SuperUserController, :login)
  end

  # Authenticated API routes
  scope "/api", LazypockWeb do
    pipe_through(:auth)

    get("/superusers/me", SuperUserController, :me)

    # File routes — must be BEFORE dynamic :collection routes
    post("/files", FileController, :upload)
    get("/files/:id", FileController, :show)
    delete("/files/:id", FileController, :delete)
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
