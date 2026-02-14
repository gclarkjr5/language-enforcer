use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{HeaderMap, HeaderValue, Request, StatusCode},
    response::{Response},
    routing::post,
    Json, Router,
};
use dotenvy::dotenv;
use serde::{Deserialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
struct AppState {
    auth_url: String,
    proxy_target: String,
    proxy_client: reqwest::Client,
    allowed_origin: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EmailAuthRequest {
    email: String,
    password: String,
    name: Option<String>,
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    let auth_url = std::env::var("NEON_AUTH_URL").expect("NEON_AUTH_URL not set");
    let bind_addr = std::env::var("BIND_ADDR").expect("BIND_ADDR not set");
    let allowed_origin = std::env::var("ALLOWED_ORIGIN").expect("ALLOWED_ORIGIN not set");
    let allowed_origin_log = allowed_origin.clone();
    let proxy_target = std::env::var("PROXY_TARGET").expect("PROXY_TARGET not set");
    let proxy_insecure = std::env::var("PROXY_INSECURE").expect("PROXY_INSECURE must be set true/false").parse().expect("Cannot parse to bool");

    let cors = {
        let origin = allowed_origin.parse::<HeaderValue>().expect("invalid ALLOWED_ORIGIN");
        CorsLayer::new()
            .allow_origin(origin)
            .allow_methods(Any)
            .allow_headers(Any)
    };
    // else {
    //     CorsLayer::new()
    //         .allow_origin(Any)
    //         .allow_methods(Any)
    //         .allow_headers(Any)
    // };

    let proxy_client = reqwest::Client::builder()
        .danger_accept_invalid_certs(proxy_insecure)
        .build()
        .expect("failed to build proxy client");

    let state = Arc::new(AppState {
        auth_url,
        proxy_target: proxy_target.clone(),
        proxy_client,
        allowed_origin: Some(allowed_origin),
    });

    let app = Router::new()
        .route("/auth/sign-in", post(sign_in))
        .route("/auth/sign-up", post(sign_up))
        .fallback(proxy_request)
        .with_state(state)
        .layer(cors);

    let addr: SocketAddr = bind_addr.parse().expect("invalid BIND_ADDR");
    println!(
        "auth-server listening on http://{addr} (proxy_target={proxy_target}, insecure={proxy_insecure}, allowed_origin={})",
        allowed_origin_log
    );
    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();
}

async fn sign_in(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<EmailAuthRequest>,
) -> Result<Json<Value>, StatusCode> {
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let sign_in_url = join_url(&state.auth_url, "/sign-in/email");
    let mut auth_req = client.post(sign_in_url).json(&json!({
        "email": payload.email,
        "password": payload.password
    }));
    if let Some(origin) = &state.allowed_origin {
        auth_req = auth_req.header("origin", origin);
    }
    let auth_resp = auth_req
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let status = auth_resp.status();
    let raw = auth_resp
        .json::<Value>()
        .await
        .unwrap_or_else(|_| json!({ "error": "invalid response from auth server" }));

    if !status.is_success() {
        return Ok(Json(json!({
            "error": raw,
            "access_token": null,
            "user": null,
            "raw": raw
        })));
    }

    let access_token = fetch_jwt(&client, &state.auth_url).await;
    let user = raw
        .get("data")
        .and_then(|data| data.get("user"))
        .cloned()
        .or_else(|| raw.get("user").cloned());

    Ok(Json(json!({
        "access_token": access_token,
        "user": user,
        "raw": raw
    })))
}

async fn sign_up(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<EmailAuthRequest>,
) -> Result<Json<Value>, StatusCode> {
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let sign_up_url = join_url(&state.auth_url, "/sign-up/email");
    let mut auth_req = client.post(sign_up_url).json(&json!({
        "email": payload.email,
        "password": payload.password,
        "name": payload.name
    }));
    if let Some(origin) = &state.allowed_origin {
        auth_req = auth_req.header("origin", origin);
    }
    let auth_resp = auth_req
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let status = auth_resp.status();
    let raw = auth_resp
        .json::<Value>()
        .await
        .unwrap_or_else(|_| json!({ "error": "invalid response from auth server" }));

    if !status.is_success() {
        return Ok(Json(json!({
            "error": raw,
            "access_token": null,
            "user": null,
            "raw": raw
        })));
    }

    let access_token = fetch_jwt(&client, &state.auth_url).await;
    let user = raw
        .get("data")
        .and_then(|data| data.get("user"))
        .cloned()
        .or_else(|| raw.get("user").cloned());

    Ok(Json(json!({
        "access_token": access_token,
        "user": user,
        "raw": raw
    })))
}

async fn fetch_jwt(client: &reqwest::Client, auth_url: &str) -> Option<String> {
    let token_url = join_url(auth_url, "/token");
    let resp = client.get(token_url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data = resp.json::<Value>().await.ok()?;
    data.get("token").and_then(|v| v.as_str()).map(|v| v.to_string())
}

fn join_url(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'))
}

async fn proxy_request(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response, StatusCode> {
    let (parts, body) = req.into_parts();
    let uri = parts.uri;
    let method = parts.method;
    let headers = parts.headers;

    let path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let target = format!("{}{}", state.proxy_target.trim_end_matches('/'), path);
    eprintln!("[proxy] {} {}", method, target);

    let body_bytes = to_bytes(body, usize::MAX)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let mut builder = state.proxy_client.request(method, target);
    builder = builder.headers(filter_proxy_headers(&headers));
    let resp = builder
        .body(body_bytes)
        .send()
        .await
        .map_err(|err| {
            eprintln!("[proxy] upstream error: {err}");
            StatusCode::BAD_GATEWAY
        })?;

    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let resp_body = resp
        .bytes()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let mut response = Response::new(Body::from(resp_body));
    *response.status_mut() = status;
    *response.headers_mut() = filter_response_headers(&resp_headers);
    Ok(response)
}

fn filter_proxy_headers(headers: &HeaderMap) -> HeaderMap {
    let mut filtered = HeaderMap::new();
    for (name, value) in headers.iter() {
        if name.as_str().eq_ignore_ascii_case("host") {
            continue;
        }
        filtered.append(name, value.clone());
    }
    filtered
}

fn filter_response_headers(headers: &HeaderMap) -> HeaderMap {
    let mut filtered = HeaderMap::new();
    for (name, value) in headers.iter() {
        if name.as_str().eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        filtered.append(name, value.clone());
    }
    filtered
}
