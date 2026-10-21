use super::*;
use reqwest::blocking::{RequestBuilder, Response};
use std::{collections::BTreeSet, io::Read, path::Path};

const SETUP_SCOPES: [&str; 5] = [
    "knowledge:read",
    "knowledge:write",
    "brains:access",
    "account:manage",
    "brains:create",
];
const CLIENT_NAME: &str = "Cortex CLI";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentPreview {
    client_name: String,
    redirect_uri: String,
    scopes: Vec<String>,
    account_access: bool,
    brain_connection: bool,
    organization: Option<Value>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    version: u8,
    origin: String,
    email: String,
    scopes: Vec<String>,
    expires_at: u64,
    phase: Phase,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Phase {
    AwaitingCode {
        challenge: String,
    },
    Session {
        cookie: String,
    },
    Authorization {
        cookie: String,
        attempt: Attempt,
    },
    Code {
        cookie: String,
        attempt: Attempt,
        code: String,
    },
    Exchanging {
        cookie: String,
        attempt: Attempt,
    },
    Cleanup {
        cookie: String,
        client_id: String,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Attempt {
    client_id: String,
    redirect_uri: String,
    verifier: String,
    state: String,
}

fn pending_path(path: &Path) -> PathBuf {
    path.with_extension("pending.json")
}

fn protocol(message: &str) -> Error {
    Error::new("protocol", message)
}

fn save(path: &Path, pending: &Pending) -> Result<()> {
    config::atomic_write(&pending_path(path), &serde_json::to_vec(pending)?)
}

fn load(origin: &str, path: &Path) -> Result<Option<Pending>> {
    let path = pending_path(path);
    config::reject_symlink(&path)?;
    if !path.exists() {
        return Ok(None);
    }
    let pending: Pending = serde_json::from_slice(&fs::read(path)?).map_err(|_| {
        Error::new(
            "authentication",
            "Invalid pending login. Run cortex login --cancel.",
        )
    })?;
    if pending.version != 1 || pending.origin != origin {
        return Err(Error::new(
            "authentication",
            "Pending login origin or version mismatch",
        ));
    }
    approved_scopes(&pending.scopes.join(" "))?;
    Ok(Some(pending))
}

pub(super) fn require_no_pending(path: &Path) -> Result<()> {
    config::reject_symlink(&pending_path(path))?;
    if pending_path(path).exists() {
        return Err(Error::new(
            "conflict",
            "An email login is pending. Run cortex login --resume or --cancel first.",
        ));
    }
    Ok(())
}

pub(super) fn requested(options: &Login) -> bool {
    options.email.is_some()
        || options.approve_scopes.is_some()
        || options.code_stdin
        || options.resume
        || options.cancel
}

fn approved_scopes(value: &str) -> Result<Vec<String>> {
    let scopes = value.split_whitespace().collect::<BTreeSet<_>>();
    if !scopes.contains("knowledge:read")
        || !scopes.contains("brains:access")
        || scopes.iter().any(|scope| !SETUP_SCOPES.contains(scope))
        || (scopes.contains("brains:create") && !scopes.contains("account:manage"))
    {
        return Err(Error::invalid(
            "Email login requires knowledge:read and brains:access; approved setup scopes may also include knowledge:write, account:manage, and brains:create. Brain creation requires account:manage.",
        ));
    }
    Ok(scopes.into_iter().map(String::from).collect())
}

fn email(value: &str) -> Result<String> {
    let value = value.trim().to_ascii_lowercase();
    let mut parts = value.split('@');
    if value.len() > 254
        || value
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        || parts.next().is_none_or(str::is_empty)
        || parts.next().is_none_or(str::is_empty)
        || parts.next().is_some()
    {
        return Err(Error::invalid("Enter a valid email address"));
    }
    Ok(value)
}

fn code_from_stdin() -> Result<String> {
    let mut bytes = Vec::new();
    std::io::stdin().lock().take(9).read_to_end(&mut bytes)?;
    if bytes.ends_with(b"\r\n") {
        bytes.truncate(bytes.len() - 2);
    } else if bytes.ends_with(b"\n") {
        bytes.pop();
    }
    if bytes.len() != 6 || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(Error::invalid(
            "Stdin must contain exactly six ASCII digits and an optional newline",
        ));
    }
    String::from_utf8(bytes).map_err(|_| Error::invalid("Invalid email code"))
}

fn response(request: RequestBuilder, action: &str) -> Result<Response> {
    request.send().map_err(|_| {
        Error::new(
            "network",
            format!("{action} did not complete. Run cortex login --resume to recover."),
        )
    })
}

fn value(response: Response, action: &str) -> Result<Value> {
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        let code = match status {
            400 | 401 => "authentication",
            403 => "permission_denied",
            409 => "conflict",
            429 => "rate_limited",
            _ => "service",
        };
        let mut error = Error::new(
            code,
            format!(
                "{action} failed. A code may have expired or already been used. Wrong codes are not retried; after five incorrect attempts wait 15 minutes. Use --resume, or --cancel before requesting a fresh code."
            ),
        );
        error.status = Some(status);
        return Err(error);
    }
    response
        .json()
        .map_err(|_| protocol("Unexpected login response"))
}

fn browser(
    client: &Client,
    origin: &str,
    method: reqwest::Method,
    path: &str,
    cookie: Option<&str>,
) -> RequestBuilder {
    let request = client
        .request(method, format!("{origin}{path}"))
        .header("Origin", origin);
    match cookie {
        Some(cookie) => request.header("Cookie", cookie),
        None => request,
    }
}

fn discover(client: &Client, origin: &str) -> Result<()> {
    let discovery = value(
        response(
            client.get(format!(
                "{origin}/.well-known/oauth-authorization-server/app/api/oauth"
            )),
            "Service discovery",
        )?,
        "Service discovery",
    )?;
    for (key, suffix) in [
        ("issuer", "/app/api/oauth"),
        ("authorization_endpoint", "/app/connect"),
        ("token_endpoint", "/app/api/oauth/token"),
        ("registration_endpoint", "/app/api/oauth/register"),
    ] {
        if text(&discovery, key)? != format!("{origin}{suffix}") {
            return Err(protocol(
                "OAuth metadata does not match the selected service origin",
            ));
        }
    }
    Ok(())
}

fn awaiting(pending: &Pending) -> Value {
    let domain = pending.email.split('@').nth(1).unwrap_or("");
    json!({"status": if pending.expires_at > now() { "awaiting_code" } else { "new_code_required" }, "signed_in": false, "email": format!("***@{domain}"), "expires_at": pending.expires_at, "scopes": pending.scopes})
}

fn cookie(response: &Response, origin: &str) -> Result<String> {
    let cookies = response
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok())
        .filter(|header| header.starts_with("second_brain_session="))
        .collect::<Vec<_>>();
    if cookies.len() != 1 {
        return Err(protocol("Expected one private sign-in session"));
    }
    let fields = cookies[0].split(';').map(str::trim).collect::<Vec<_>>();
    let token = fields[0]
        .strip_prefix("second_brain_session=")
        .unwrap_or("");
    if token.is_empty()
        || token.len() > 512
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        || !fields.contains(&"Path=/app/api")
        || !fields.contains(&"HttpOnly")
        || !fields.contains(&"SameSite=Strict")
        || (origin.starts_with("https:") && !fields.contains(&"Secure"))
    {
        return Err(protocol("Invalid private sign-in session"));
    }
    Ok(fields[0].into())
}

fn session_user(client: &Client, pending: &Pending, cookie: &str) -> Result<String> {
    let session = value(
        response(
            browser(
                client,
                &pending.origin,
                reqwest::Method::GET,
                "/app/api/session",
                Some(cookie),
            ),
            "Account verification",
        )?,
        "Account verification",
    )?;
    if session.get("authenticated") != Some(&json!(true))
        || session.pointer("/user/email").and_then(Value::as_str) != Some(pending.email.as_str())
    {
        return Err(protocol(
            "The signed-in email does not match this login request. Cancel this login.",
        ));
    }
    config::identifier(&text(&session["user"], "id")?, "usr")
        .map_err(|_| protocol("Invalid signed-in identity"))
}

fn register(client: &Client, origin: &str) -> Result<Attempt> {
    let redirect_uri = format!("http://127.0.0.1/callback/{}", uuid::Uuid::new_v4());
    let registration = value(response(client.post(format!("{origin}/app/api/oauth/register")).json(&json!({"client_name": CLIENT_NAME, "redirect_uris": [&redirect_uri], "token_endpoint_auth_method": "none", "grant_types": ["authorization_code", "refresh_token"], "response_types": ["code"]})), "Client registration")?, "Client registration")?;
    Ok(Attempt {
        client_id: config::identifier(&text(&registration, "client_id")?, "ocl")
            .map_err(|_| protocol("Invalid OAuth client identity"))?,
        redirect_uri,
        verifier: format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ),
        state: uuid::Uuid::new_v4().to_string(),
    })
}

fn authorization(pending: &Pending, attempt: &Attempt) -> Value {
    json!({"client_id": attempt.client_id, "redirect_uri": attempt.redirect_uri, "resource": format!("{}/app/api/mcp", pending.origin), "response_type": "code", "scope": pending.scopes.join(" "), "state": attempt.state, "code_challenge_method": "S256", "code_challenge": URL_SAFE_NO_PAD.encode(Sha256::digest(attempt.verifier.as_bytes()))})
}

fn consent(client: &Client, pending: &Pending, cookie: &str, attempt: &Attempt) -> Result<String> {
    session_user(client, pending, cookie)?;
    let input = authorization(pending, attempt);
    let preview = value(
        response(
            browser(
                client,
                &pending.origin,
                reqwest::Method::GET,
                "/app/api/oauth/consent",
                Some(cookie),
            )
            .query(&input),
            "Permission verification",
        )?,
        "Permission verification",
    )?;
    let preview: ConsentPreview = serde_json::from_value(preview)
        .map_err(|_| protocol("Unexpected permission preview response"))?;
    if preview.client_name != CLIENT_NAME
        || preview.redirect_uri != attempt.redirect_uri
        || approved_scopes(&preview.scopes.join(" "))? != pending.scopes
        || !preview.account_access
        || preview.brain_connection
        || preview.organization.is_some()
    {
        return Err(protocol(
            "The consent request does not match the approved account connection",
        ));
    }
    let mut approval = input;
    approval["approved"] = json!(true);
    let approved = value(
        response(
            browser(
                client,
                &pending.origin,
                reqwest::Method::POST,
                "/app/api/oauth/consent",
                Some(cookie),
            )
            .json(&approval),
            "Permission approval",
        )?,
        "Permission approval",
    )?;
    let redirect = reqwest::Url::parse(&text(&approved, "redirect")?)
        .map_err(|_| protocol("Invalid OAuth callback"))?;
    let expected = reqwest::Url::parse(&attempt.redirect_uri)
        .map_err(|_| protocol("Invalid private callback"))?;
    if redirect.scheme() != expected.scheme()
        || redirect.host_str() != expected.host_str()
        || redirect.port_or_known_default() != expected.port_or_known_default()
        || redirect.path() != expected.path()
        || !redirect.username().is_empty()
        || redirect.password().is_some()
        || redirect.fragment().is_some()
        || redirect
            .query_pairs()
            .any(|(key, _)| !["state", "iss", "code"].contains(&key.as_ref()))
    {
        return Err(protocol(
            "OAuth callback does not match the registered callback",
        ));
    }
    callback_code(
        &redirect,
        &attempt.state,
        &format!("{}/app/api/oauth", pending.origin),
    )
}

fn installed(origin: &str, path: &Path, client: &Client) -> Result<Value> {
    let mut credentials = read_credentials(origin, path)?;
    if credentials.expires_at <= now().saturating_add(60) {
        let tokens = value(
            response(
                client.post(format!("{origin}/app/api/oauth/token")).form(&[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", credentials.refresh_token.as_str()),
                    ("client_id", credentials.client_id.as_str()),
                    ("resource", &format!("{origin}/app/api/mcp")),
                ]),
                "Connection refresh",
            )?,
            "Connection refresh",
        )?;
        save_tokens(path, &mut credentials, tokens)?;
    }
    value(
        response(
            client
                .get(format!("{origin}/app/api/cli/account"))
                .bearer_auth(&credentials.access_token),
            "Connection verification",
        )?,
        "Connection verification",
    )?;
    Ok(
        json!({"status": "signed_in", "signed_in": true, "origin": origin, "scopes": credentials.scope.split_whitespace().collect::<Vec<_>>() }),
    )
}

fn cleanup(origin: &str, cookie: &str, client: &Client) -> Result<()> {
    let result = response(
        browser(
            client,
            origin,
            reqwest::Method::DELETE,
            "/app/api/session",
            Some(cookie),
        ),
        "Temporary session cleanup",
    )?;
    if ![204, 401].contains(&result.status().as_u16()) {
        value(result, "Temporary session cleanup")?;
    }
    Ok(())
}

pub(super) fn cancel(origin: &str, path: &Path, client: &Client) -> Result<()> {
    if let Some(pending) = load(origin, path)? {
        match &pending.phase {
            Phase::AwaitingCode { .. } => {}
            Phase::Session { cookie }
            | Phase::Authorization { cookie, .. }
            | Phase::Code { cookie, .. }
            | Phase::Exchanging { cookie, .. }
            | Phase::Cleanup { cookie, .. } => cleanup(origin, cookie, client)?,
        }
        fs::remove_file(pending_path(path))?;
    }
    Ok(())
}

fn discard_invalid(origin: &str, path: &Path) -> Result<bool> {
    let path = pending_path(path);
    config::reject_symlink(&path)?;
    if !path.exists() {
        return Ok(false);
    }
    let bytes = fs::read(&path)?;
    let valid = serde_json::from_slice::<Pending>(&bytes).is_ok_and(|pending| {
        pending.version == 1
            && pending.origin == origin
            && approved_scopes(&pending.scopes.join(" ")).is_ok()
    });
    if valid {
        return Ok(false);
    }
    fs::remove_file(path)?;
    Ok(true)
}

fn continue_login(
    origin: &str,
    path: &Path,
    client: &Client,
    mut pending: Pending,
) -> Result<Value> {
    loop {
        let published = match &pending.phase {
            Phase::Authorization { attempt, .. }
            | Phase::Code { attempt, .. }
            | Phase::Exchanging { attempt, .. } => read_credentials(origin, path)
                .is_ok_and(|credentials| credentials.client_id == attempt.client_id),
            _ => false,
        };
        if published {
            let (cookie, client_id) = match pending.phase {
                Phase::Authorization { cookie, attempt }
                | Phase::Code {
                    cookie, attempt, ..
                }
                | Phase::Exchanging { cookie, attempt } => (cookie, attempt.client_id),
                _ => unreachable!(),
            };
            pending.phase = Phase::Cleanup { cookie, client_id };
            save(path, &pending)?;
        }
        if pending.expires_at <= now() && !matches!(&pending.phase, Phase::Cleanup { .. }) {
            return Err(Error::new(
                "authentication",
                "Pending login expired. Run cortex login --cancel, then request a fresh code.",
            ));
        }
        match &pending.phase {
            Phase::AwaitingCode { .. } => return Ok(awaiting(&pending)),
            Phase::Session { cookie } => {
                session_user(client, &pending, cookie)?;
                let attempt = register(client, origin)?;
                pending.phase = Phase::Authorization {
                    cookie: cookie.clone(),
                    attempt,
                };
            }
            Phase::Authorization { cookie, attempt } => {
                let code = consent(client, &pending, cookie, attempt)?;
                let Phase::Authorization { cookie, attempt } = pending.phase else {
                    unreachable!()
                };
                pending.phase = Phase::Code {
                    cookie,
                    attempt,
                    code,
                };
            }
            Phase::Code { .. } => {
                let Phase::Code {
                    cookie,
                    attempt,
                    code,
                } = pending.phase
                else {
                    unreachable!()
                };
                pending.phase = Phase::Exchanging { cookie, attempt };
                save(path, &pending)?;
                let Phase::Exchanging { cookie, attempt } = &pending.phase else {
                    unreachable!()
                };
                let tokens = value(
                    response(
                        client.post(format!("{origin}/app/api/oauth/token")).form(&[
                            ("grant_type", "authorization_code"),
                            ("code", code.as_str()),
                            ("client_id", attempt.client_id.as_str()),
                            ("redirect_uri", attempt.redirect_uri.as_str()),
                            ("code_verifier", attempt.verifier.as_str()),
                            ("resource", &format!("{origin}/app/api/mcp")),
                        ]),
                        "Connection authorization",
                    )?,
                    "Connection authorization",
                )?;
                let scope = text(&tokens, "scope")?;
                if approved_scopes(&scope)? != pending.scopes {
                    return Err(protocol(
                        "Granted permissions do not match the explicitly approved scopes",
                    ));
                }
                let mut credentials = Credentials {
                    origin: origin.into(),
                    client_id: attempt.client_id.clone(),
                    redirect_uri: attempt.redirect_uri.clone(),
                    access_token: String::new(),
                    refresh_token: String::new(),
                    expires_at: 0,
                    scope,
                };
                save_tokens(path, &mut credentials, tokens)?;
                pending.phase = Phase::Cleanup {
                    cookie: cookie.clone(),
                    client_id: attempt.client_id.clone(),
                };
            }
            Phase::Exchanging { .. } => {
                let Phase::Exchanging {
                    cookie,
                    mut attempt,
                } = pending.phase
                else {
                    unreachable!()
                };
                attempt.verifier = format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                );
                attempt.state = uuid::Uuid::new_v4().to_string();
                pending.phase = Phase::Authorization { cookie, attempt };
            }
            Phase::Cleanup { cookie, client_id } => {
                if read_credentials(origin, path)?.client_id != *client_id {
                    return Err(Error::new(
                        "conflict",
                        "Installed connection changed. Cancel pending cleanup without replacing credentials.",
                    ));
                }
                cleanup(origin, cookie, client)?;
                fs::remove_file(pending_path(path))?;
                return installed(origin, path, client);
            }
        }
        save(path, &pending)?;
    }
}

pub(super) fn login(origin: &str, options: &Login) -> Result<Value> {
    let modes = usize::from(options.email.is_some())
        + usize::from(options.code_stdin)
        + usize::from(options.resume)
        + usize::from(options.cancel);
    if modes != 1
        || (options.email.is_some() != options.approve_scopes.is_some())
        || options.no_browser
    {
        return Err(Error::invalid(
            "Choose login --email EMAIL --approve-scopes PERMISSIONS, --code-stdin, --resume, or --cancel. Browser flags do not apply.",
        ));
    }
    let code = if options.code_stdin {
        Some(code_from_stdin()?)
    } else {
        None
    };
    let path = credential_path(origin)?;
    let _guard = config::lock(&path.with_extension("lock"))?;
    let client = client(origin)?;
    if options.cancel {
        if discard_invalid(origin, &path)? {
            return Ok(
                json!({"status": "pending_cancelled", "signed_in": false, "credentials_preserved": true, "temporary_session_cleanup": "unknown", "message": "Invalid local pending state was removed. Remote session cleanup cannot be confirmed; an existing session may last seven days. Installed credentials are preserved."}),
            );
        }
        cancel(origin, &path, &client)?;
        return Ok(
            json!({"status": "pending_cancelled", "signed_in": false, "credentials_preserved": true}),
        );
    }
    let pending = load(origin, &path)?;
    if let Some(input) = &options.email {
        let email = email(input)?;
        let scopes = approved_scopes(options.approve_scopes.as_deref().unwrap_or(""))?;
        if let Some(pending) = pending {
            if pending.email == email && pending.scopes == scopes {
                return if matches!(&pending.phase, Phase::AwaitingCode { .. }) {
                    Ok(awaiting(&pending))
                } else {
                    Err(Error::new(
                        "conflict",
                        "Email verification already progressed. Run cortex login --resume.",
                    ))
                };
            }
            return Err(Error::new(
                "conflict",
                "Another email login is pending. Resume or cancel it before starting another.",
            ));
        }
        discover(&client, origin)?;
        let result = value(
            response(
                browser(
                    &client,
                    origin,
                    reqwest::Method::POST,
                    "/app/api/session",
                    None,
                )
                .json(&json!({"email": email})),
                "Sending sign-in code",
            )?,
            "Sending sign-in code",
        )?;
        let pending = Pending {
            version: 1,
            origin: origin.into(),
            email,
            scopes,
            expires_at: now().saturating_add(900),
            phase: Phase::AwaitingCode {
                challenge: text(&result, "challenge")?,
            },
        };
        save(&path, &pending)?;
        return Ok(awaiting(&pending));
    }
    let Some(mut pending) = pending else {
        return installed(origin, &path, &client);
    };
    if let Phase::AwaitingCode { challenge } = &pending.phase {
        let Some(code) = code else {
            return Ok(awaiting(&pending));
        };
        if pending.expires_at <= now() {
            return Ok(awaiting(&pending));
        }
        let accepted = response(
            browser(
                &client,
                origin,
                reqwest::Method::POST,
                "/app/api/auth/code",
                None,
            )
            .json(&json!({"challenge": challenge, "code": code})),
            "Code verification",
        )?;
        if !accepted.status().is_success() {
            value(accepted, "Code verification")?;
            unreachable!();
        }
        let cookie = cookie(&accepted, origin)?;
        pending.phase = Phase::Session { cookie };
        save(&path, &pending)?;
        value(accepted, "Code verification")?;
    }
    discover(&client, origin)?;
    continue_login(origin, &path, &client, pending)
}
