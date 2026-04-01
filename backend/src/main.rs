mod extensions;
mod protocol;
mod workspace;

use std::net::SocketAddr;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use extensions::{ExtensionRegistry, InstallExtensionParams};
use futures::{SinkExt, StreamExt};
use protocol::{RpcRequest, RpcResponse};
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{error, info};
use workspace::{OpenFileParams, SaveFileParams, WorkspaceStore};

#[derive(Clone)]
struct AppState {
    workspace: WorkspaceStore,
    extensions: ExtensionRegistry,
    events: broadcast::Sender<Value>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("hematite_backend=debug,info")
        .init();

    let (events, _) = broadcast::channel(512);
    let state = AppState {
        workspace: WorkspaceStore::default(),
        extensions: ExtensionRegistry::default(),
        events,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/rpc", get(ws_rpc))
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = "127.0.0.1:8989".parse()?;
    info!(%addr, "Hematite backend running");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<Value> {
    Json(json!({"ok": true, "service": "hematite-backend"}))
}

async fn ws_rpc(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(state, socket))
}

async fn handle_ws(state: AppState, mut socket: WebSocket) {
    let mut events_rx = state.events.subscribe();

    loop {
        tokio::select! {
            Some(incoming) = socket.next() => {
                match incoming {
                    Ok(Message::Text(raw)) => {
                        match serde_json::from_str::<RpcRequest>(&raw) {
                            Ok(request) => {
                                let response = dispatch(&state, request).await;
                                match serde_json::to_string(&response) {
                                    Ok(payload) => {
                                        if let Err(err) = socket.send(Message::Text(payload)).await {
                                            error!(%err, "failed to send rpc response");
                                            return;
                                        }
                                    }
                                    Err(err) => {
                                        error!(%err, "failed to serialize rpc response");
                                    }
                                }
                            }
                            Err(err) => {
                                let response = RpcResponse::err(json!(null), -32700, format!("parse error: {err}"));
                                let _ = socket.send(Message::Text(serde_json::to_string(&response).unwrap_or_else(|_| "{}".into()))).await;
                            }
                        }
                    }
                    Ok(Message::Close(_)) => return,
                    Ok(_) => {},
                    Err(err) => {
                        error!(%err, "websocket error");
                        return;
                    }
                }
            }
            Ok(event) = events_rx.recv() => {
                let message = json!({"id": null, "result": {"event": event}});
                if let Err(err) = socket.send(Message::Text(message.to_string())).await {
                    error!(%err, "failed to push event");
                    return;
                }
            }
        }
    }
}

async fn dispatch(state: &AppState, req: RpcRequest) -> RpcResponse {
    match req.method.as_str() {
        "workspace/open" => {
            let Ok(params) = serde_json::from_value::<OpenFileParams>(req.params.clone()) else {
                return RpcResponse::err(req.id, -32602, "invalid params for workspace/open");
            };

            state.workspace.open(params.path.clone(), params.content).await;
            let _ = state.events.send(json!({"type":"fileOpened", "path": params.path}));
            RpcResponse::ok(req.id, json!({"success": true}))
        }
        "workspace/save" => {
            let Ok(params) = serde_json::from_value::<SaveFileParams>(req.params.clone()) else {
                return RpcResponse::err(req.id, -32602, "invalid params for workspace/save");
            };

            state.workspace.save(params.path.clone(), params.content).await;
            let _ = state.events.send(json!({"type":"fileSaved", "path": params.path}));
            RpcResponse::ok(req.id, json!({"success": true}))
        }
        "workspace/read" => {
            let path = req.params.get("path").and_then(Value::as_str).unwrap_or_default();
            let content = state.workspace.get(path).await.unwrap_or_default();
            RpcResponse::ok(req.id, json!({"path": path, "content": content}))
        }
        "extensions/install" => {
            let Ok(params) = serde_json::from_value::<InstallExtensionParams>(req.params.clone()) else {
                return RpcResponse::err(req.id, -32602, "invalid params for extensions/install");
            };

            let extension = state.extensions.install(params).await;
            let _ = state.events.send(json!({"type":"extensionInstalled", "extension": extension}));
            RpcResponse::ok(req.id, json!({"extension": extension}))
        }
        "extensions/list" => {
            let extensions = state.extensions.list().await;
            RpcResponse::ok(req.id, json!({"extensions": extensions}))
        }
        "capabilities" => RpcResponse::ok(
            req.id,
            json!({
                "vscode_compat": {
                    "extension_host": "planned",
                    "lsp": true,
                    "dap": true,
                    "theme_api": "planned"
                },
                "transport": "json-rpc-over-websocket"
            }),
        ),
        _ => RpcResponse::err(req.id, -32601, format!("method not found: {}", req.method)),
    }
}
