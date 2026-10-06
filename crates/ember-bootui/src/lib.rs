//! Optional local operations dashboard for Ember applications.
//!
//! Add this crate explicitly when a browser-based operations view is wanted.
//! Ember core does not install or expose the dashboard by default.

use axum::{response::Html, routing::get, Router};
pub use ember_config::BootUiConfig;
use std::net::SocketAddr;

inventory::submit! {
    ember_core::OptionalExtensionRegistration { name: "ember-bootui", start }
}

/// Starts BootUI when it is enabled in `application.yml` or
/// `application.properties`.
pub fn start() {
    let config = match ember_config::ConfigLoader::new().load::<ember_config::EmberConfig>() {
        Ok(config) => config.bootui,
        Err(_) => return,
    };
    if !config.enabled {
        return;
    }

    let address = format!("{}:{}", config.host, config.port);
    let path = config.path.clone();
    tokio::spawn(async move {
        let Ok(address) = address.parse::<SocketAddr>() else {
            tracing::error!("BootUI address is invalid");
            return;
        };
        let listener = match tokio::net::TcpListener::bind(address).await {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!(
                    %address,
                    error_kind = "bootui_bind",
                    "BootUI could not bind its configured port: {error}"
                );
                return;
            }
        };
        tracing::info!(target: "ember::startup", %address, path = %path, "Ember BootUI started");
        if let Err(error) = axum::serve(listener, layer(Router::new(), &config)).await {
            tracing::error!("BootUI stopped unexpectedly: {error}");
        }
    });
}

/// Adds the BootUI dashboard to an existing Ember router.
///
/// The dashboard is only added when `bootui.enabled` is true. The application
/// should bind its server to the configured local host when `local_only` is
/// enabled; the dashboard itself never changes the application's bind policy.
pub fn layer(router: Router, config: &BootUiConfig) -> Router {
    if !config.enabled {
        return router;
    }

    let path = config.path.trim_end_matches('/');
    let dashboard = Router::new()
        .route("/", get(index))
        .route("/api/config", get(config_snapshot));
    router.nest(path, dashboard)
}

async fn index() -> Html<&'static str> {
    Html(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Ember BootUI</title>
  <style>
    :root { color-scheme: dark; font-family: Inter, system-ui, sans-serif; }
    body { margin: 0; background: #111827; color: #f9fafb; }
    header { padding: 28px 36px; background: linear-gradient(135deg,#241205,#7c2d12); }
    h1 { margin: 0 0 6px; color: #fb923c; }
    main { padding: 28px 36px; display: grid; gap: 18px; grid-template-columns: repeat(auto-fit,minmax(240px,1fr)); }
    section { padding: 20px; border: 1px solid #374151; border-radius: 14px; background: #1f2937; }
    code, pre { color: #fdba74; white-space: pre-wrap; }
    .muted { color: #9ca3af; }
  </style>
</head>
<body>
  <header><h1>🔥 Ember BootUI</h1><div class="muted">Local application operations dashboard</div></header>
  <main>
    <section><h2>Application</h2><pre id="config">Loading...</pre></section>
    <section><h2>Endpoints</h2><p><a id="health" href="#">Health</a></p><p><a id="live" href="#">Live</a></p><p><a id="ready" href="#">Ready</a></p><p><a id="metrics" href="#">Metrics</a></p></section>
    <section><h2>Status</h2><p class="muted">Use the actuator endpoints for live health data.</p></section>
  </main>
  <script>
    const base = window.location.pathname.replace(/\/$/, '');
    fetch(base + '/api/config').then(response => response.json()).then(value => {
      document.getElementById('config').textContent = JSON.stringify(value, null, 2);
      const server = `http://${value.server.host}:${value.server.port}`;
      document.getElementById('health').href = server + '/health';
      document.getElementById('live').href = server + '/live';
      document.getElementById('ready').href = server + '/ready';
      document.getElementById('metrics').href = server + '/metrics';
    }).catch(error => document.getElementById('config').textContent = String(error));
  </script>
</body>
</html>"##,
    )
}

async fn config_snapshot() -> axum::Json<serde_json::Value> {
    let loader = ember_config::ConfigLoader::new();
    match loader.load::<ember_config::EmberConfig>() {
        Ok(config) => axum::Json(serde_json::to_value(config).unwrap_or_else(
            |_| serde_json::json!({"error": "configuration could not be serialized"}),
        )),
        Err(_) => axum::Json(serde_json::json!({"error": "configuration could not be loaded"})),
    }
}
