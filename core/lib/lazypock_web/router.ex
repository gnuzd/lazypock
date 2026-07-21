defmodule LazypockWeb.Router do
  use LazypockWeb, :router

  pipeline :browser do
    plug :accepts, ["html"]
    plug :fetch_session
    plug :fetch_live_flash
    plug :put_root_layout, html: {LazypockWeb.Layouts, :root}
    plug :protect_from_forgery
    plug :put_secure_browser_headers
  end

  pipeline :api do
    plug :accepts, ["json"]
  end

  scope "/", LazypockWeb do
    pipe_through :browser

    get "/", PageController, :home
  end

  # API routes
  scope "/api", LazypockWeb do
    pipe_through :api

    # Health check
    get "/health", HealthController, :index

    # Dynamic collection routes — must be LAST
    # Matches: GET /api/posts, GET /api/posts/:id, POST /api/posts, etc.
    get "/:collection", DynamicController, :list
    get "/:collection/:id", DynamicController, :show
    post "/:collection", DynamicController, :create
    patch "/:collection/:id", DynamicController, :update
    put "/:collection/:id", DynamicController, :update
    delete "/:collection/:id", DynamicController, :delete
  end
end
