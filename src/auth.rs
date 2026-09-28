use crate::{
    args::Login,
    config,
    error::{Error, Result},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, Deserialize)]
pub struct Credentials {
    origin: String,
    client_id: String,
    redirect_uri: String,
    access_token: String,
    refresh_token: String,
    expires_at: u64,
    scope: String,
}

pub fn client(origin: &str) -> Result<Client> {
    let url = reqwest::Url::parse(origin).map_err(|_| Error::invalid("Invalid service origin"))?;
    let mut builder = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(45))
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("second-brain-cli/", env!("CARGO_PKG_VERSION")));
    if let Some(host) = url.host_str()
        && (host == "localhost" || host.ends_with(".localhost"))
    {
        builder = builder.no_proxy().resolve(
            host,
            std::net::SocketAddr::from(([127, 0, 0, 1], url.port_or_known_default().unwrap_or(80))),
        );
    }
    Ok(builder.build()?)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn credential_path(origin: &str) -> Result<PathBuf> {
    let key = URL_SAFE_NO_PAD.encode(Sha256::digest(origin.as_bytes()));
    Ok(config::auth_dir()?.join(format!("{key}.json")))
}

pub fn decode(response: reqwest::blocking::Response) -> Result<Value> {
    let status = response.status().as_u16();
    let value = response.json::<Value>().map_err(|_| {
        Error::new(
            "protocol",
            "Service returned an unexpected response; verify the origin and API version",
        )
    })?;
    if !(200..300).contains(&status) {
        let code = match status {
            401 => "authentication",
            403 => "permission_denied",
            409 => "conflict",
            429 => "rate_limited",
            _ => "service",
        };
        let mut error = Error::new(
            code,
            value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Service operation failed"),
        );
        error.status = Some(status);
        return Err(error);
    }
    Ok(value)
}

fn text(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .ok_or_else(|| Error::new("protocol", "OAuth response is missing a required field"))
}

fn save_tokens(path: &std::path::Path, credentials: &mut Credentials, tokens: Value) -> Result<()> {
    if !tokens
        .get("token_type")
        .and_then(Value::as_str)
        .is_some_and(|s| s.eq_ignore_ascii_case("bearer"))
    {
        return Err(Error::new("protocol", "Unsupported OAuth token type"));
    }
    credentials.access_token = text(&tokens, "access_token")?;
    credentials.refresh_token = text(&tokens, "refresh_token")?;
    let expires = tokens
        .get("expires_in")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::new("protocol", "Missing OAuth token expiry"))?;
    credentials.expires_at = now().saturating_add(expires);
    if let Some(scope) = tokens.get("scope").and_then(Value::as_str) {
        credentials.scope = scope.into();
    }
    config::atomic_write(path, &serde_json::to_vec(credentials)?)
}

pub fn access_token(origin: &str, client: &Client) -> Result<String> {
    let path = credential_path(origin)?;
    let _guard = config::lock(&path.with_extension("lock"))?;
    let mut credentials = read_credentials(origin, &path)?;
    if credentials.expires_at <= now() + 60 {
        let response = client
            .post(format!("{origin}/app/api/oauth/token"))
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &credentials.refresh_token),
                ("client_id", &credentials.client_id),
                ("resource", &format!("{origin}/app/api/mcp")),
            ])
            .send()?;
        let tokens = decode(response).map_err(|_| Error::new("authentication", "Could not refresh this connection. Run brain login again; preserve pending request IDs."))?;
        save_tokens(&path, &mut credentials, tokens)?;
    }
    Ok(credentials.access_token)
}

fn read_credentials(origin: &str, path: &std::path::Path) -> Result<Credentials> {
    config::reject_symlink(path)?;
    let bytes = fs::read(path).map_err(|_| {
        Error::new(
            "authentication",
            "No CLI credentials for this origin. Run brain login.",
        )
    })?;
    let credentials: Credentials = serde_json::from_slice(&bytes).map_err(|_| {
        Error::new(
            "authentication",
            "Invalid local credential file. Run brain login.",
        )
    })?;
    if credentials.origin != origin {
        return Err(Error::new("authentication", "Credential origin mismatch"));
    }
    Ok(credentials)
}

pub fn logout(origin: &str) -> Result<Value> {
    let path = credential_path(origin)?;
    let _guard = config::lock(&path.with_extension("lock"))?;
    if !path.exists() {
        return Ok(json!({"signed_out": true}));
    }
    let credentials = read_credentials(origin, &path)?;
    decode(
        client(origin)?
            .post(format!("{origin}/app/api/oauth/revoke"))
            .form(&[
                ("token", credentials.refresh_token),
                ("client_id", credentials.client_id),
            ])
            .send()?,
    )?;
    fs::remove_file(path)?;
    Ok(json!({"signed_out": true}))
}

pub fn login(origin: &str, options: &Login) -> Result<Value> {
    if !(1..=1800).contains(&options.timeout) {
        return Err(Error::invalid("Login timeout must be 1-1800 seconds"));
    }
    let scopes = options.scopes.split_whitespace().collect::<Vec<_>>();
    if !scopes.contains(&"knowledge:read") || !scopes.contains(&"brains:access") {
        return Err(Error::invalid(
            "CLI login requires knowledge:read and brains:access",
        ));
    }
    let path = credential_path(origin)?;
    let _guard = config::lock(&path.with_extension("lock"))?;
    let client = client(origin)?;
    let discovery = decode(
        client
            .get(format!(
                "{origin}/.well-known/oauth-authorization-server/app/api/oauth"
            ))
            .send()?,
    )?;
    for (key, expected) in [
        ("issuer", format!("{origin}/app/api/oauth")),
        ("authorization_endpoint", format!("{origin}/app/connect")),
        ("token_endpoint", format!("{origin}/app/api/oauth/token")),
        (
            "registration_endpoint",
            format!("{origin}/app/api/oauth/register"),
        ),
    ] {
        if text(&discovery, key)? != expected {
            return Err(Error::new(
                "protocol",
                "OAuth metadata does not match the selected service origin",
            ));
        }
    }
    let server = tiny_http::Server::http("127.0.0.1:0")
        .map_err(|_| Error::new("io", "Could not start the OAuth loopback listener"))?;
    let port = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| Error::new("io", "Missing loopback address"))?
        .port();
    let callback_path = format!("/callback/{}", uuid::Uuid::new_v4());
    let redirect_uri = format!("http://127.0.0.1:{port}{callback_path}");
    let registration = decode(client.post(format!("{origin}/app/api/oauth/register")).json(&json!({
        "client_name": "Second Brain CLI (local evaluation)", "redirect_uris": [redirect_uri],
        "token_endpoint_auth_method": "none", "grant_types": ["authorization_code", "refresh_token"], "response_types": ["code"]
    })).send()?)?;
    let client_id = text(&registration, "client_id")?;
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = uuid::Uuid::new_v4().to_string();
    let resource = format!("{origin}/app/api/mcp");
    let issuer = format!("{origin}/app/api/oauth");
    let mut authorization = reqwest::Url::parse(&format!("{origin}/app/connect"))
        .map_err(|_| Error::invalid("Invalid origin"))?;
    authorization.query_pairs_mut().extend_pairs([
        ("client_id", client_id.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("resource", resource.as_str()),
        ("response_type", "code"),
        ("scope", options.scopes.as_str()),
        ("state", state.as_str()),
        ("code_challenge_method", "S256"),
        ("code_challenge", challenge.as_str()),
    ]);
    eprintln!(
        "{}",
        json!({"event": "authorization_required", "url": authorization.as_str(), "expires_in": options.timeout})
    );
    if !options.no_browser {
        open_browser(authorization.as_str());
    }
    let deadline = Instant::now() + Duration::from_secs(options.timeout);
    while Instant::now() < deadline {
        let Some(request) = server.recv_timeout(Duration::from_millis(250))? else {
            continue;
        };
        let callback = reqwest::Url::parse(&format!("http://127.0.0.1:{port}{}", request.url()));
        let code = callback.ok().and_then(|url| {
            if request.method() != &tiny_http::Method::Get || url.path() != callback_path {
                return None;
            }
            callback_code(&url, &state, &issuer).ok()
        });
        let Some(code) = code else {
            let _ = request.respond(
                tiny_http::Response::from_string(
                    "Invalid OAuth callback. Return to the consent page.",
                )
                .with_status_code(400),
            );
            continue;
        };
        let tokens = decode(
            client
                .post(format!("{origin}/app/api/oauth/token"))
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("code", code.as_str()),
                    ("client_id", client_id.as_str()),
                    ("redirect_uri", redirect_uri.as_str()),
                    ("code_verifier", verifier.as_str()),
                    ("resource", resource.as_str()),
                ])
                .send()?,
        );
        match tokens {
            Ok(tokens) => {
                let mut credentials = Credentials {
                    origin: origin.into(),
                    client_id,
                    redirect_uri,
                    access_token: String::new(),
                    refresh_token: String::new(),
                    expires_at: 0,
                    scope: String::new(),
                };
                save_tokens(&path, &mut credentials, tokens)?;
                let _ = request.respond(tiny_http::Response::from_string(
                    "Signed in to Second Brain. Return to your agent or terminal.",
                ));
                return Ok(
                    json!({"signed_in": true, "origin": origin, "scopes": credentials.scope.split_whitespace().collect::<Vec<_>>() }),
                );
            }
            Err(error) => {
                let _ = request.respond(
                    tiny_http::Response::from_string("Sign-in failed. Return to the terminal.")
                        .with_status_code(400),
                );
                return Err(error);
            }
        }
    }
    Err(Error::new(
        "authentication",
        "Login timed out. Run brain login again.",
    ))
}

pub fn callback_code(url: &reqwest::Url, state: &str, issuer: &str) -> Result<String> {
    let pairs = url.query_pairs().collect::<Vec<_>>();
    for key in ["state", "iss", "code"] {
        if pairs.iter().filter(|(name, _)| name == key).count() != 1 {
            return Err(Error::new(
                "authentication",
                "Invalid OAuth callback parameters",
            ));
        }
    }
    let get = |key| {
        pairs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_ref())
    };
    if get("state") != Some(state)
        || get("iss") != Some(issuer)
        || get("code").is_none_or(str::is_empty)
        || get("error").is_some()
    {
        return Err(Error::new(
            "authentication",
            "OAuth state or issuer mismatch",
        ));
    }
    Ok(get("code").unwrap().into())
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let command = "open";
    #[cfg(not(target_os = "macos"))]
    let command = "xdg-open";
    if std::process::Command::new(command)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_err()
    {
        eprintln!("Open the authorization URL above to continue.");
    }
}
