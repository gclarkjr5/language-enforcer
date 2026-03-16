use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderMap, HeaderValue, Request, StatusCode},
    middleware::{Next, from_fn},
    response::Response,
    routing::post,
};
use dotenvy::dotenv;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

#[derive(Clone)]
struct AppState {
    auth_url: String,
    proxy_target: Option<String>,
    proxy_client: Option<reqwest::Client>,
    allowed_origin: Vec<String>,
    openai_key: Option<String>,
    openai_model: String,
    openai_client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct EmailAuthRequest {
    email: String,
    password: String,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GenerateSentenceRequest {
    word: String,
    translation: Option<String>,
    source_language: String,
    target_language: String,
    concept: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GenerateQuestionRequest {
    word: String,
    translation: Option<String>,
    source_language: String,
    target_language: String,
    concept: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GradeSentenceRequest {
    word: String,
    target_language: String,
    user_sentence: String,
    question: Option<String>,
    concept: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CleanupEntry {
    word_id: String,
    text: String,
    translation: Option<String>,
    language: String,
    sentence: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CleanupRequest {
    entries: Vec<CleanupEntry>,
}

#[derive(Debug, Serialize)]
struct CleanupSuggestion {
    word_id: String,
    text: String,
    language: String,
    current_translation: Option<String>,
    suggestion: String,
    notes: Option<String>,
}

#[derive(Debug, Serialize)]
struct CleanupResponse {
    suggestions: Vec<CleanupSuggestion>,
}

#[derive(Debug, Serialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    temperature: f32,
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    let auth_url = std::env::var("NEON_AUTH_URL").expect("NEON_AUTH_URL not set");
    let bind_addr = std::env::var("BIND_ADDR").expect("BIND_ADDR not set");
    let allowed_origin = std::env::var("ALLOWED_ORIGIN").ok();
    let allowed_origin_list: Vec<String> = allowed_origin
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    let proxy_target = std::env::var("PROXY_TARGET").ok();
    let proxy_insecure = std::env::var("PROXY_INSECURE")
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false);
    let openai_key = std::env::var("OPENAI_API_KEY").ok();
    let openai_model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());

    let cors = if allowed_origin_list.is_empty() {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        let origins = allowed_origin_list
            .iter()
            .map(|origin| {
                origin
                    .parse::<HeaderValue>()
                    .expect("invalid ALLOWED_ORIGIN")
            })
            .collect::<Vec<_>>();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods(Any)
            .allow_headers(Any)
    };

    let proxy_client = proxy_target.as_ref().map(|_| {
        reqwest::Client::builder()
            .danger_accept_invalid_certs(proxy_insecure)
            .build()
            .expect("failed to build proxy client")
    });

    let state = Arc::new(AppState {
        auth_url,
        proxy_target: proxy_target.clone(),
        proxy_client,
        allowed_origin: allowed_origin_list,
        openai_key,
        openai_model,
        openai_client: reqwest::Client::new(),
    });

    let app = Router::new()
        .route("/auth/sign-in", post(sign_in))
        .route("/auth/sign-up", post(sign_up))
        .route("/ai/generate-sentence", post(generate_sentence))
        .route("/ai/generate-question", post(generate_question))
        .route("/ai/cleanup", post(cleanup_translations))
        .route("/ai/grade-sentence", post(grade_sentence))
        .fallback(proxy_request)
        .with_state(state.clone())
        .layer(from_fn(log_request))
        .layer(cors);

    let addr: SocketAddr = bind_addr.parse().expect("invalid BIND_ADDR");
    println!(
        "auth-server listening on http://{addr} (proxy_target={}, insecure={proxy_insecure}, allowed_origin={})",
        proxy_target
            .clone()
            .unwrap_or_else(|| "disabled".to_string()),
        if state.allowed_origin.is_empty() {
            "any".to_string()
        } else {
            state.allowed_origin.join(",")
        }
    );
    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();
}

async fn log_request(req: Request<Body>, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let response = next.run(req).await;
    println!("[request] {} {} -> {}", method, uri, response.status());
    response
}

async fn sign_in(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<EmailAuthRequest>,
) -> Result<Json<Value>, StatusCode> {
    println!("[auth] sign-in request");
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let sign_in_url = join_url(&state.auth_url, "/sign-in/email");
    let mut auth_req = client.post(sign_in_url).json(&json!({
        "email": payload.email,
        "password": payload.password
    }));
    if let Some(origin) = state.allowed_origin.first() {
        auth_req = auth_req.header("origin", origin);
    }
    let auth_resp = auth_req.send().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

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
    println!("[auth] sign-up request");
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
    if let Some(origin) = state.allowed_origin.first() {
        auth_req = auth_req.header("origin", origin);
    }
    let auth_resp = auth_req.send().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

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
    data.get("token")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
}

async fn generate_sentence(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GenerateSentenceRequest>,
) -> Result<Json<Value>, StatusCode> {
    let Some(key) = state.openai_key.as_ref() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let translation_hint = payload
        .translation
        .as_ref()
        .map(|value| value.as_str())
        .unwrap_or("none");
    let concept = sanitize_concept(&payload.concept);
    let concept_note = concept
        .as_ref()
        .map(|value| {
            format!(
                " Focus on incorporating the concept \"{value}\".",
                value = value
            )
        })
        .unwrap_or_default();
    let system = "Return a compact JSON object with keys \"sentence\" and \"translation\". No markdown. Both the sentence and translation should read like a CEFR B1-level example.";
    let user = format!(
        "Create a natural {source} sentence using the word \"{word}\" at CEFR B1 level. Provide its {target} translation written with B1-level vocabulary and grammar. Translation hint: {hint}.{concept_note}",
        source = payload.source_language,
        target = payload.target_language,
        word = payload.word,
        hint = translation_hint,
        concept_note = concept_note
    );
    let content = call_openai(&state, key, system, &user).await?;
    let data: Value = serde_json::from_str(&content).map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(data))
}

async fn generate_question(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GenerateQuestionRequest>,
) -> Result<Json<Value>, StatusCode> {
    let Some(key) = state.openai_key.as_ref() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let concept = sanitize_concept(&payload.concept);
    let concept_note = concept
        .as_ref()
        .map(|value| format!(" Include the concept \"{value}\" in the question so the learner can use both the word and that construction.", value = value))
        .unwrap_or_default();
    let system = "Return a compact JSON object with key \"question\". No markdown. Compose the question in Dutch at CEFR B1 level and ensure it clearly asks the learner to respond with a sentence that uses the provided word and, when available, the highlighted concept.";
    let user = format!(
        "Using the word \"{word}\" ({source}), craft a Dutch CEFR B1 question that mentions both the word and the concept, then ask the learner to reply with a Dutch sentence featuring them. {concept_note} Respond only with the question itself.",
        source = payload.source_language,
        word = payload.word,
        concept_note = concept_note
    );
    let content = call_openai(&state, key, system, &user).await?;
    let data: Value = serde_json::from_str(&content).map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(data))
}

async fn cleanup_translations(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CleanupRequest>,
) -> Result<Json<CleanupResponse>, StatusCode> {
    let Some(key) = state.openai_key.as_ref() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    if payload.entries.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut suggestions = Vec::new();
    for entry in payload.entries.iter().take(10) {
        let translation_hint = entry
            .translation
            .as_ref()
            .map(|value| value.as_str())
            .unwrap_or("none");
        let context = entry
            .sentence
            .as_ref()
            .map(|value| value.as_str())
            .unwrap_or("no example sentence provided");
        let system = "Return a compact JSON object with keys \"suggestion\" and \"notes\" only. No markdown. Keep the translation text CEFR B1-level, fully in English, and avoid repeating the Dutch input or wrapping it in parentheses.";
        let user = format!(
            "Review the current translation for the Dutch word \"{word}\" ({language}) with the existing suggestion \"{translation}\". Context: {context}. Provide only an improved English translation; do not include any Dutch words or remark about the original. Keep the translation natural and indicate your reasoning under \"notes\".",
            word = entry.text,
            language = entry.language,
            translation = translation_hint,
            context = context
        );
        let content = call_openai(&state, key, system, &user).await?;
        let data: Value = serde_json::from_str(&content).map_err(|_| StatusCode::BAD_GATEWAY)?;
        let suggestion = data
            .get("suggestion")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let notes = data
            .get("notes")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        suggestions.push(CleanupSuggestion {
            word_id: entry.word_id.clone(),
            text: entry.text.clone(),
            language: entry.language.clone(),
            current_translation: entry.translation.clone(),
            suggestion,
            notes,
        });
    }
    Ok(Json(CleanupResponse { suggestions }))
}

async fn grade_sentence(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GradeSentenceRequest>,
) -> Result<Json<Value>, StatusCode> {
    let Some(key) = state.openai_key.as_ref() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let question_context = payload
        .question
        .as_ref()
        .map(|q| format!(" Question: \"{q}\"."))
        .unwrap_or_default();
    let concept = sanitize_concept(&payload.concept);
    let concept_context = concept
        .as_ref()
        .map(|value| {
            format!(
                " Consider the concept \"{value}\" when evaluating the sentence.",
                value = value
            )
        })
        .unwrap_or_default();
    let system = "Return a compact JSON object with keys \"score\" (1-10), \"feedback\" (very short), and \"correction\" (a corrected sentence). No markdown. Focus on a CEFR B1-level evaluation.";
    let user = format!(
        "Evaluate the user's {language} sentence for correct use of the word \"{word}\". Sentence: \"{sentence}\".{question_context}{concept_context} Provide a B1-level score (1-10), describe the issue in a concise rubric, and, if needed, offer a B1-level corrected sentence as the \"correction\" value.",
        language = payload.target_language,
        word = payload.word,
        sentence = payload.user_sentence,
        question_context = question_context,
        concept_context = concept_context
    );
    let content = call_openai(&state, key, system, &user).await?;
    let data: Value = serde_json::from_str(&content).map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(data))
}

async fn call_openai(
    state: &AppState,
    key: &str,
    system: &str,
    user: &str,
) -> Result<String, StatusCode> {
    let req = OpenAIRequest {
        model: state.openai_model.clone(),
        messages: vec![
            OpenAIMessage {
                role: "system".to_string(),
                content: system.to_string(),
            },
            OpenAIMessage {
                role: "user".to_string(),
                content: user.to_string(),
            },
        ],
        temperature: 0.7,
    };
    let resp = state
        .openai_client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(key)
        .json(&req)
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        eprintln!("[openai] error status={status} body={body}");
        return Err(StatusCode::BAD_GATEWAY);
    }
    let data: Value = resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let content = data
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|value| value.as_str())
        .ok_or(StatusCode::BAD_GATEWAY)?;
    Ok(content.trim().to_string())
}

fn join_url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn sanitize_concept(concept: &Option<String>) -> Option<String> {
    concept
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.replace('"', "'"))
}

async fn proxy_request(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response, StatusCode> {
    let Some(proxy_target) = state.proxy_target.as_ref() else {
        return Err(StatusCode::NOT_FOUND);
    };
    let Some(proxy_client) = state.proxy_client.as_ref() else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let (parts, body) = req.into_parts();
    let uri = parts.uri;
    let method = parts.method;
    let headers = parts.headers;

    let path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let target = format!("{}{}", proxy_target.trim_end_matches('/'), path);
    eprintln!("[proxy] {} {}", method, target);

    let body_bytes = to_bytes(body, usize::MAX)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let mut builder = proxy_client.request(method, target);
    builder = builder.headers(filter_proxy_headers(&headers));
    let resp = builder.body(body_bytes).send().await.map_err(|err| {
        eprintln!("[proxy] upstream error: {err}");
        StatusCode::BAD_GATEWAY
    })?;

    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let resp_body = resp.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

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
