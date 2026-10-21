use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use tempfile::TempDir;

const SCOPES: &str = "knowledge:read knowledge:write brains:access account:manage brains:create";
const SORTED: &str = "account:manage brains:access brains:create knowledge:read knowledge:write";

#[derive(Default)]
struct State {
    fault: String,
    requests: Vec<String>,
    approval: Value,
    exchanges: usize,
    accepts: usize,
    accepted: bool,
    cleanups: usize,
}

struct Fixture {
    root: TempDir,
    origin: String,
    state: Arc<Mutex<State>>,
    stopped: Arc<AtomicBool>,
    server: Option<thread::JoinHandle<()>>,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("project")).unwrap();
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let origin = format!(
            "http://127.0.0.1:{}",
            server.server_addr().to_ip().unwrap().port()
        );
        let state = Arc::new(Mutex::new(State::default()));
        let stopped = Arc::new(AtomicBool::new(false));
        let (service, shared, stopping) = (origin.clone(), state.clone(), stopped.clone());
        let worker = thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                let Some(mut request) = server.recv_timeout(Duration::from_millis(20)).unwrap()
                else {
                    continue;
                };
                let url = reqwest::Url::parse(&format!("{service}{}", request.url())).unwrap();
                let path = url.path();
                let method = request.method().to_string();
                let mut raw = String::new();
                request.as_reader().read_to_string(&mut raw).unwrap();
                let input: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
                let mut state = shared.lock().unwrap();
                state.requests.push(format!("{method} {path}"));
                if path == "/app/api/session"
                    || path == "/app/api/auth/code"
                    || path == "/app/api/oauth/consent"
                {
                    assert!(
                        request
                            .headers()
                            .iter()
                            .any(|header| header.field.equiv("Origin")
                                && header.value.as_str() == service)
                    );
                }
                if (path == "/app/api/session" && method != "POST")
                    || path == "/app/api/oauth/consent"
                {
                    assert!(
                        request
                            .headers()
                            .iter()
                            .any(|header| header.field.equiv("Cookie")
                                && header.value.as_str()
                                    == "second_brain_session=synthetic-private-session")
                    );
                } else {
                    assert!(
                        !request
                            .headers()
                            .iter()
                            .any(|header| header.field.equiv("Cookie"))
                    );
                }
                let mut cookie = false;
                let (status, result) = match (method.as_str(), path) {
                    ("GET", "/.well-known/oauth-authorization-server/app/api/oauth") => (
                        200,
                        json!({"issuer": if state.fault == "discovery" { "https://other.test/app/api/oauth".into() } else { format!("{service}/app/api/oauth") }, "authorization_endpoint": format!("{service}/app/connect"), "token_endpoint": format!("{service}/app/api/oauth/token"), "registration_endpoint": format!("{service}/app/api/oauth/register")}),
                    ),
                    ("POST", "/app/api/session") => {
                        assert_eq!(input["email"], "alex@example.com");
                        state.accepted = false;
                        (
                            200,
                            json!({"challenge": "synthetic-private-challenge", "developmentCode": "001234", "message": "Check your email"}),
                        )
                    }
                    ("POST", "/app/api/auth/code") => {
                        state.accepts += 1;
                        assert_eq!(input["challenge"], "synthetic-private-challenge");
                        if input["code"] != "001234" || state.fault == "expired" || state.accepted {
                            (
                                400,
                                json!({"error": "synthetic-private-challenge 001234 synthetic-private-session"}),
                            )
                        } else {
                            state.accepted = true;
                            cookie = true;
                            if state.fault == "lost_acceptance" {
                                cookie = false;
                                (503, json!({}))
                            } else {
                                (200, json!({"authenticated": true}))
                            }
                        }
                    }
                    ("GET", "/app/api/session") => (
                        200,
                        json!({"authenticated": true, "user": {"id": "usr_1111111111111111", "email": if state.fault == "email" { "other@example.com" } else { "alex@example.com" }}}),
                    ),
                    ("DELETE", "/app/api/session") => {
                        state.cleanups += 1;
                        if state.fault == "cleanup" {
                            (503, json!({}))
                        } else if state.fault == "already_cleaned" {
                            (401, json!({}))
                        } else {
                            (204, Value::Null)
                        }
                    }
                    ("POST", "/app/api/oauth/register") => {
                        let redirect =
                            reqwest::Url::parse(input["redirect_uris"][0].as_str().unwrap())
                                .unwrap();
                        assert_eq!(redirect.port(), None);
                        assert_eq!(redirect.host_str(), Some("127.0.0.1"));
                        state.approval = input;
                        (201, json!({"client_id": "ocl_1111111111111111"}))
                    }
                    ("GET", "/app/api/oauth/consent") => {
                        let mut authorization = json!({});
                        for (key, val) in url.query_pairs() {
                            authorization[key.as_ref()] = json!(val);
                        }
                        assert_eq!(authorization["client_id"], "ocl_1111111111111111");
                        assert_eq!(authorization["resource"], format!("{service}/app/api/mcp"));
                        let scopes = if state.fault == "consent_scope" {
                            "knowledge:read organization:manage"
                        } else {
                            SORTED
                        };
                        let result = json!({"clientName": if state.fault == "consent_client" { "Other client" } else { "Cortex CLI" }, "redirectUri": authorization["redirect_uri"], "organization": null, "organizations": [], "scopes": scopes.split_whitespace().collect::<Vec<_>>(), "canCreateBrain": true, "accountAccess": true, "brainConnection": state.fault == "consent_resource"});
                        (200, result)
                    }
                    ("POST", "/app/api/oauth/consent") => {
                        assert_eq!(input["approved"], true);
                        assert_eq!(input["scope"], SORTED);
                        assert_eq!(input["resource"], format!("{service}/app/api/mcp"));
                        let mut redirect =
                            reqwest::Url::parse(input["redirect_uri"].as_str().unwrap()).unwrap();
                        if state.fault == "callback_path" {
                            redirect.set_path("/different");
                        }
                        if state.fault == "callback_host" {
                            redirect.set_host(Some("localhost")).unwrap();
                        }
                        if state.fault == "callback_fragment" {
                            redirect.set_fragment(Some("secret"));
                        }
                        redirect
                            .query_pairs_mut()
                            .append_pair(
                                "state",
                                if state.fault == "callback_state" {
                                    "wrong"
                                } else {
                                    input["state"].as_str().unwrap()
                                },
                            )
                            .append_pair(
                                "iss",
                                &if state.fault == "callback_issuer" {
                                    "https://other.test/app/api/oauth".into()
                                } else {
                                    format!("{service}/app/api/oauth")
                                },
                            )
                            .append_pair("code", "synthetic-private-oauth-code");
                        if state.fault == "callback_duplicate" {
                            redirect.query_pairs_mut().append_pair("code", "duplicate");
                        }
                        (200, json!({"redirect": redirect.as_str()}))
                    }
                    ("POST", "/app/api/oauth/token") => {
                        state.exchanges += 1;
                        if state.fault == "exchange" {
                            (503, json!({}))
                        } else {
                            (
                                200,
                                json!({"access_token": "synthetic-private-access", "refresh_token": "synthetic-private-refresh", "expires_in": 3600, "token_type": "Bearer", "scope": if state.fault == "token_scope" { "knowledge:read brains:access organization:manage" } else { SORTED }}),
                            )
                        }
                    }
                    ("GET", "/app/api/cli/account") => (
                        200,
                        json!({"user": {"id": "usr_1111111111111111", "email": "alex@example.com"}}),
                    ),
                    ("POST", "/app/api/oauth/revoke") => (200, json!({})),
                    _ => panic!("Unexpected route {method} {path}"),
                };
                let mut response = tiny_http::Response::from_string(result.to_string())
                    .with_status_code(status)
                    .with_header(
                        tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap(),
                    );
                if cookie {
                    response.add_header(tiny_http::Header::from_bytes("Set-Cookie", "second_brain_session=synthetic-private-session; Path=/app/api; HttpOnly; SameSite=Strict; Max-Age=604800").unwrap());
                }
                let _ = request.respond(response);
            }
        });
        Self {
            root,
            origin,
            state,
            stopped,
            server: Some(worker),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cortex"));
        command
            .args(["--origin", &self.origin])
            .current_dir(self.root.path().join("project"))
            .env("CORTEX_HOME", self.root.path().join("auth"))
            .env_remove("CORTEX_ORIGIN")
            .env_remove("BRAIN_ORIGIN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn run(&self, args: &[&str], input: &[u8]) -> (i32, Value) {
        let mut process = self.command().args(args).spawn().unwrap();
        process.stdin.take().unwrap().write_all(input).unwrap();
        self.decode(process.wait_with_output().unwrap())
    }

    fn decode(&self, output: Output) -> (i32, Value) {
        let text = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.is_empty(), "{stderr}");
        for secret in [
            "001234",
            "synthetic-private",
            "authorization_required",
            "code_challenge",
            "127.0.0.1/callback",
        ] {
            assert!(!text.contains(secret), "{text}");
        }
        (
            output.status.code().unwrap(),
            serde_json::from_str(&text).unwrap(),
        )
    }

    fn begin(&self) -> Value {
        let (code, result) = self.run(
            &[
                "login",
                "--email",
                "Alex@example.com",
                "--approve-scopes",
                SCOPES,
            ],
            b"",
        );
        assert_eq!(code, 0, "{result}");
        assert_eq!(result["data"]["status"], "awaiting_code");
        result
    }

    fn private_file(&self, pending: bool) -> PathBuf {
        fs::read_dir(self.root.path().join("auth"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.to_string_lossy().ends_with("pending.json") == pending
                    && path.extension().is_some_and(|value| value == "json")
            })
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.server.take().unwrap().join().unwrap();
    }
}

#[test]
fn chat_login_survives_separate_processes_and_keeps_secrets_private() {
    let fixture = Fixture::new();
    fixture.begin();
    assert_eq!(
        fixture.run(&["login", "--resume"], b"").1["data"]["status"],
        "awaiting_code"
    );
    let pending = fixture.private_file(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&pending).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(pending.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    let (code, result) = fixture.run(&["login", "--code-stdin"], b"001234\n");
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["data"]["signed_in"], true);
    assert_eq!(result["data"]["selection"]["reason"], "email_login");
    assert!(!pending.exists());
    assert!(!fixture.root.path().join("project/.cortex").exists());
    assert_eq!(fixture.run(&["account", "show"], b"").0, 0);
    assert_eq!(fixture.run(&["login", "--resume"], b"").0, 0);
    assert_eq!(fixture.run(&["login", "--code-stdin"], b"001234").0, 0);
    assert_eq!(fixture.state.lock().unwrap().exchanges, 1);
    assert_eq!(fixture.state.lock().unwrap().accepts, 1);
    assert_eq!(fixture.state.lock().unwrap().cleanups, 1);
}

#[test]
fn wrong_and_expired_codes_stay_pending_without_retries_or_secret_errors() {
    let fixture = Fixture::new();
    fixture.begin();
    assert_ne!(fixture.run(&["login", "--code-stdin"], b"999999").0, 0);
    assert_eq!(fixture.state.lock().unwrap().accepts, 1);
    fixture.state.lock().unwrap().fault = "expired".into();
    assert_ne!(fixture.run(&["login", "--code-stdin"], b"001234").0, 0);
    assert_eq!(
        fixture.run(&["login", "--resume"], b"").1["data"]["status"],
        "awaiting_code"
    );
    let path = fixture.private_file(true);
    let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["expires_at"] = json!(1);
    fs::write(&path, state.to_string()).unwrap();
    assert_eq!(
        fixture.run(&["login", "--resume"], b"").1["data"]["status"],
        "new_code_required"
    );
    assert_eq!(
        fixture.run(&["login", "--code-stdin"], b"001234").1["data"]["status"],
        "new_code_required"
    );
    assert_eq!(fixture.state.lock().unwrap().accepts, 2);
}

#[test]
fn invalid_modes_codes_and_scope_approval_do_no_network_work() {
    let fixture = Fixture::new();
    for args in [
        vec!["login", "--email", "alex@example.com"],
        vec!["login", "--approve-scopes", SCOPES],
        vec!["login", "--resume", "--cancel"],
        vec![
            "login",
            "--email",
            "alex@example.com",
            "--approve-scopes",
            "knowledge:read brains:access organization:manage",
        ],
        vec!["login", "--code-stdin", "--no-browser"],
    ] {
        assert_ne!(fixture.run(&args, b"").0, 0);
    }
    for code in [
        b"12345".as_slice(),
        b"1234567",
        b" 123456",
        b"123456\n\n",
        b"12345x",
        b"12345678901234567890",
    ] {
        assert_ne!(fixture.run(&["login", "--code-stdin"], code).0, 0);
    }
    assert!(fixture.state.lock().unwrap().requests.is_empty());
}

#[test]
fn pending_intent_cannot_be_silently_replaced_by_email_or_browser_login() {
    let fixture = Fixture::new();
    fixture.begin();
    let count = fixture.state.lock().unwrap().requests.len();
    fixture.begin();
    assert_ne!(
        fixture
            .run(
                &[
                    "login",
                    "--email",
                    "other@example.com",
                    "--approve-scopes",
                    SCOPES
                ],
                b""
            )
            .0,
        0
    );
    assert_ne!(fixture.run(&["login", "--no-browser"], b"").0, 0);
    assert_eq!(fixture.state.lock().unwrap().requests.len(), count);
    let (code, result) = fixture.run(&["login", "--cancel"], b"");
    assert_eq!(code, 0, "{result}");
    fixture.begin();
}

#[test]
fn discovery_consent_identity_callback_and_token_mismatches_fail_closed() {
    for fault in [
        "discovery",
        "email",
        "consent_scope",
        "consent_client",
        "consent_resource",
        "callback_path",
        "callback_host",
        "callback_fragment",
        "callback_state",
        "callback_issuer",
        "callback_duplicate",
        "token_scope",
    ] {
        let fixture = Fixture::new();
        fixture.begin();
        fixture.state.lock().unwrap().fault = fault.into();
        let (code, result) = fixture.run(&["login", "--code-stdin"], b"001234");
        assert_ne!(code, 0, "{fault}: {result}");
        assert!(
            !fs::read_dir(fixture.root.path().join("auth"))
                .unwrap()
                .any(|entry| {
                    let path = entry.unwrap().path();
                    path.extension().is_some_and(|ext| ext == "json")
                        && !path.to_string_lossy().ends_with("pending.json")
                })
        );
        assert_eq!(
            fixture.state.lock().unwrap().exchanges,
            usize::from(fault == "token_scope")
        );
        assert_eq!(fixture.run(&["login", "--cancel"], b"").0, 0);
    }
}

#[test]
fn saved_session_and_ambiguous_exchange_resume_without_another_email_code() {
    let fixture = Fixture::new();
    fixture.begin();
    fixture.state.lock().unwrap().fault = "exchange".into();
    assert_ne!(fixture.run(&["login", "--code-stdin"], b"001234").0, 0);
    assert_eq!(fixture.state.lock().unwrap().accepts, 1);
    fixture.state.lock().unwrap().fault.clear();
    let (code, result) = fixture.run(&["login", "--resume"], b"");
    assert_eq!(code, 0, "{result}");
    assert_eq!(fixture.state.lock().unwrap().accepts, 1);
    assert_eq!(fixture.state.lock().unwrap().exchanges, 2);
}

#[test]
fn tokens_are_durable_before_retryable_cleanup_and_logout_handles_pending() {
    let fixture = Fixture::new();
    fixture.begin();
    fixture.state.lock().unwrap().fault = "cleanup".into();
    assert_ne!(fixture.run(&["login", "--code-stdin"], b"001234").0, 0);
    assert_eq!(fixture.run(&["account", "show"], b"").0, 0);
    assert_ne!(fixture.run(&["login", "--cancel"], b"").0, 0);
    assert_ne!(fixture.run(&["logout"], b"").0, 0);
    assert!(fixture.private_file(true).exists());
    fixture.state.lock().unwrap().fault = "already_cleaned".into();
    assert_eq!(fixture.run(&["login", "--resume"], b"").0, 0);
    assert_eq!(fixture.state.lock().unwrap().exchanges, 1);
    let credential = fixture.private_file(false);
    fixture.begin();
    assert_eq!(fixture.run(&["logout"], b"").0, 0);
    assert!(!credential.exists());
}

#[test]
fn existing_credentials_and_project_selection_survive_failed_replacement_and_cancel() {
    let fixture = Fixture::new();
    fixture.begin();
    assert_eq!(fixture.run(&["login", "--code-stdin"], b"001234").0, 0);
    let credential = fixture.private_file(false);
    let before = fs::read(&credential).unwrap();
    fs::create_dir(fixture.root.path().join("project/.cortex")).unwrap();
    fs::write(
        fixture.root.path().join("project/.cortex/config.toml"),
        "brain_id = 'org_1111111111111111'\n",
    )
    .unwrap();
    fixture.begin();
    assert_ne!(fixture.run(&["login", "--code-stdin"], b"999999").0, 0);
    assert_eq!(fs::read(&credential).unwrap(), before);
    assert_eq!(fixture.run(&["login", "--cancel"], b"").0, 0);
    assert_eq!(fs::read(&credential).unwrap(), before);
    assert_eq!(
        fixture.run(&["config"], b"").1["data"]["brain_id"],
        "org_1111111111111111"
    );
}

#[test]
fn cancellation_recovers_corrupt_local_state_without_claiming_remote_cleanup() {
    let fixture = Fixture::new();
    fixture.begin();
    let path = fixture.private_file(true);
    fs::write(&path, b"invalid pending state").unwrap();
    assert_ne!(fixture.run(&["login", "--resume"], b"").0, 0);
    assert_ne!(fixture.run(&["logout"], b"").0, 0);
    let (code, result) = fixture.run(&["login", "--cancel"], b"");
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["data"]["temporary_session_cleanup"], "unknown");
    assert!(!path.exists());
    assert_eq!(fixture.state.lock().unwrap().cleanups, 0);
    fixture.begin();
}

#[test]
fn credential_publication_before_pending_marker_resumes_cleanup_without_reauthorization() {
    let fixture = Fixture::new();
    fixture.begin();
    fixture.state.lock().unwrap().fault = "cleanup".into();
    assert_ne!(fixture.run(&["login", "--code-stdin"], b"001234").0, 0);
    let path = fixture.private_file(true);
    let mut pending: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    pending["phase"] = json!({"kind": "exchanging", "cookie": "second_brain_session=synthetic-private-session", "attempt": {"client_id": "ocl_1111111111111111", "redirect_uri": "http://127.0.0.1/callback/test", "verifier": "synthetic-private-verifier", "state": "synthetic-private-state"}});
    fs::write(&path, pending.to_string()).unwrap();
    fixture.state.lock().unwrap().fault.clear();
    assert_eq!(fixture.run(&["login", "--resume"], b"").0, 0);
    assert_eq!(fixture.state.lock().unwrap().accepts, 1);
    assert_eq!(fixture.state.lock().unwrap().exchanges, 1);
    assert!(!path.exists());
}

#[test]
fn lost_acceptance_response_requires_a_fresh_code_without_automatic_replay() {
    let fixture = Fixture::new();
    fixture.begin();
    fixture.state.lock().unwrap().fault = "lost_acceptance".into();
    assert_ne!(fixture.run(&["login", "--code-stdin"], b"001234").0, 0);
    fixture.state.lock().unwrap().fault.clear();
    assert_eq!(
        fixture.run(&["login", "--resume"], b"").1["data"]["status"],
        "awaiting_code"
    );
    assert_eq!(fixture.state.lock().unwrap().accepts, 1);
    assert_ne!(fixture.run(&["login", "--code-stdin"], b"001234").0, 0);
    assert_eq!(fixture.run(&["login", "--cancel"], b"").0, 0);
    fixture.begin();
    assert_eq!(fixture.run(&["login", "--code-stdin"], b"001234").0, 0);
}
