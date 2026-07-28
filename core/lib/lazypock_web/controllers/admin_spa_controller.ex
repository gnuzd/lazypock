defmodule LazypockWeb.AdminSpaController do
  use LazypockWeb, :controller

  @doc """
  Serves the Svelte SPA for all /_/* routes.
  In production, the SPA is built to priv/static/admin/.
  In dev, you run vite dev server separately.
  """
  def index(conn, _params) do
    path = Path.join(:code.priv_dir(:lazypock), "static/studio/index.html")

    if File.exists?(path) do
      conn
      |> put_resp_header("content-type", "text/html")
      |> send_file(200, path)
    else
      # Dev mode: return a placeholder or redirect to vite dev server
      conn
      |> put_resp_header("content-type", "text/html")
      |> send_resp(200, dev_spa_html())
    end
  end

  defp dev_spa_html do
    """
    <!DOCTYPE html>
    <html lang="en">
    <head>
      <meta charset="UTF-8" />
      <meta name="viewport" content="width=device-width, initial-scale=1.0" />
      <title>Lazypock Admin (Dev)</title>
      <style>
        body { font-family: system-ui; display: flex; align-items: center; justify-content: center; min-height: 100vh; margin: 0; background: #1a1a2e; color: #eee; }
        .msg { text-align: center; }
        .msg p { color: #888; }
        .msg code { background: #333; padding: 2px 6px; border-radius: 4px; }
      </style>
    </head>
    <body>
      <div class="msg">
        <h2>🛠️ Lazypock Admin</h2>
        <p>SPA not built yet. Run <code>cd ui && npm run dev</code> for development.</p>
        <p>Then visit <code>http://localhost:5173/_/</code></p>
      </div>
    </body>
    </html>
    """
  end
end
