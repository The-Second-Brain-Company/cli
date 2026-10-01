use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    process::{Command, Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use tempfile::TempDir;

const FIRST: &str = "org_1111111111111111";
const SECOND: &str = "org_2222222222222222";
const RUN: &str = "run_1111111111111111";

#[test]
fn cortex_reads_legacy_selection_and_credentials_without_migrating_them() {
    let fixture = Fixture::new();
    fixture.login(&["--no-select"]);
    let project = fixture.root.path().join("project");
    let legacy = project.join(".brain");
    fs::create_dir(&legacy).unwrap();
    let content = format!("brain_id = '{FIRST}'\n");
    fs::write(legacy.join("config.toml"), &content).unwrap();
    assert_eq!(fixture.run(&["config"]).1["data"]["brain_id"], FIRST);
    assert_eq!(fixture.run(&["search", "pricing"]).0, 0);
    assert!(!project.join(".cortex").exists());
    assert_eq!(
        fs::read_to_string(legacy.join("config.toml")).unwrap(),
        content
    );
    let private = fixture.root.path().join("private-config");
    fs::create_dir(&private).unwrap();
    fs::rename(
        fixture.root.path().join("auth"),
        private.join("second-brain"),
    )
    .unwrap();
    let output = fixture
        .command()
        .env_remove("CORTEX_HOME")
        .env_remove("BRAIN_HOME")
        .env("XDG_CONFIG_HOME", &private)
        .arg("whoami")
        .output()
        .unwrap();
    assert_eq!(decode(output).0, 0);
    assert!(!private.join("cortex").exists());
    fs::rename(
        private.join("second-brain"),
        fixture.root.path().join("auth"),
    )
    .unwrap();
    assert_eq!(fixture.run(&["use", SECOND]).0, 0);
    assert_eq!(fixture.run(&["config"]).1["data"]["brain_id"], SECOND);
    assert_eq!(
        fs::read_to_string(legacy.join("config.toml")).unwrap(),
        content
    );
    fs::write(
        project.join(".cortex/config.toml"),
        "brain_id = 'invalid'\n",
    )
    .unwrap();
    assert_ne!(fixture.run(&["config"]).0, 0);
}

#[test]
fn cortex_reads_the_previous_product_named_selection_without_rewriting_it() {
    let fixture = Fixture::new();
    fixture.login(&["--no-select"]);
    let directory = fixture.root.path().join("project/.cortex");
    fs::create_dir(&directory).unwrap();
    let path = directory.join("config.toml");
    let content = format!("cortex_id = '{FIRST}'\n");
    fs::write(&path, &content).unwrap();
    let result = fixture.run(&["config"]).1;
    assert_eq!(result["data"]["brain_id"], FIRST);
    assert!(result["data"].get("cortex_id").is_none());
    assert_eq!(fixture.run(&["search", "pricing"]).0, 0);
    assert_eq!(fs::read_to_string(&path).unwrap(), content);
    assert_eq!(fixture.run(&["use", SECOND]).0, 0);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        format!("brain_id = \"{SECOND}\"\n")
    );
}

#[derive(Default)]
struct State {
    requests: Vec<(String, String, Value)>,
    challenge: String,
    organizations: Vec<Value>,
    fail_discovery: bool,
    deny_membership: bool,
    select_during_discovery: Option<std::path::PathBuf>,
    refreshes: usize,
    verification_count: usize,
    fail_verification: bool,
    wait_fails: bool,
    revoked: bool,
    response_encoding: Option<&'static str>,
}
struct Fixture {
    origin: String,
    root: TempDir,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    server: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn new() -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let origin = format!(
            "http://second-brain.localhost:{}",
            server.server_addr().to_ip().unwrap().port()
        );
        let state = Arc::new(Mutex::new(State {
            organizations: vec![json!({"id": FIRST, "name": "Fixture", "role": "owner"})],
            ..State::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, stopping, service) = (state.clone(), stop.clone(), origin.clone());
        let worker = thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                let Some(mut request) = server.recv_timeout(Duration::from_millis(20)).unwrap()
                else {
                    continue;
                };
                let url = reqwest::Url::parse(&format!("{service}{}", request.url())).unwrap();
                let path = url.path();
                let mut raw = String::new();
                request.as_reader().read_to_string(&mut raw).unwrap();
                let mut state = shared.lock().unwrap();
                let input = serde_json::from_str(&raw).unwrap_or(Value::Null);
                state
                    .requests
                    .push((request.method().to_string(), path.into(), input.clone()));
                let (status, value) = if path.contains("oauth-authorization-server") {
                    (
                        200,
                        json!({"issuer": format!("{service}/app/api/oauth"), "authorization_endpoint": format!("{service}/app/connect"), "token_endpoint": format!("{service}/app/api/oauth/token"), "registration_endpoint": format!("{service}/app/api/oauth/register")}),
                    )
                } else if path.ends_with("/oauth/register") {
                    (201, json!({"client_id": "ocl_1111111111111111"}))
                } else if path.ends_with("/oauth/token") {
                    let form = reqwest::Url::parse(&format!("http://localhost/?{raw}")).unwrap();
                    let fields = form
                        .query_pairs()
                        .collect::<std::collections::HashMap<_, _>>();
                    if fields["grant_type"] == "authorization_code" {
                        assert_eq!(
                            URL_SAFE_NO_PAD
                                .encode(Sha256::digest(fields["code_verifier"].as_bytes())),
                            state.challenge
                        );
                        assert_eq!(fields["resource"], format!("{service}/app/api/mcp"));
                    } else {
                        assert_eq!(fields["refresh_token"], "synthetic-refresh-token");
                        state.refreshes += 1;
                    }
                    (
                        200,
                        json!({"access_token": "synthetic-access-token", "refresh_token": "synthetic-refresh-token", "expires_in": if state.refreshes > 0 { 3600 } else { 1 }, "token_type": "Bearer", "scope": "knowledge:read brains:access knowledge:write organization:manage"}),
                    )
                } else if path.ends_with("/oauth/revoke") {
                    state.revoked = true;
                    (200, json!({}))
                } else if path.starts_with("/app/api/cli/") {
                    assert!(
                        request
                            .headers()
                            .iter()
                            .any(|header| header.field.equiv("Authorization")
                                && header.value.as_str() == "Bearer synthetic-access-token")
                    );
                    if path.ends_with("/membership") {
                        state.verification_count += 1;
                        if state.deny_membership
                            || (state.fail_verification
                                && state.verification_count.is_multiple_of(2))
                        {
                            (403, json!({"error": "Access changed during selection"}))
                        } else {
                            let id = if path.contains(SECOND) { SECOND } else { FIRST };
                            (
                                200,
                                json!({"organization": {"id": id, "name": "Fixture"}, "role": "owner"}),
                            )
                        }
                    } else if path.ends_with("/knowledge/retrieve") {
                        let input: Value = serde_json::from_str(
                            &url.query_pairs().find(|(key, _)| key == "input").unwrap().1,
                        )
                        .unwrap();
                        (
                            200,
                            json!({"revision": "a".repeat(40), "query": input, "results": [{"path": "policy.md", "snippet": "Fixture policy"}]}),
                        )
                    } else if path.ends_with("/knowledge/attachment") {
                        let bytes = [0_u8, 1, 255, 0];
                        let name = url
                            .query_pairs()
                            .find(|(key, _)| key == "path")
                            .unwrap()
                            .1
                            .into_owned();
                        let checksum = if name.ends_with("corrupt.bin") {
                            "bad".into()
                        } else {
                            format!("{:x}", Sha256::digest(bytes))
                        };
                        (
                            200,
                            json!({"path": name, "revision": "a".repeat(40), "data": "AAH/AA==", "byteCount": 4, "sha256": checksum}),
                        )
                    } else if path.ends_with("/knowledge/attachments")
                        || path.ends_with("/knowledge/copy")
                    {
                        (
                            200,
                            json!({"path": input["path"], "revision": "b".repeat(40)}),
                        )
                    } else if path.ends_with("/knowledge/record") {
                        (202, json!({"runId": RUN, "status": "running"}))
                    } else if path.ends_with(&format!("/runs/{RUN}")) {
                        if state.wait_fails {
                            (503, json!({"error": "Temporary wait failure"}))
                        } else {
                            (
                                200,
                                json!({"runId": RUN, "status": "saved", "receipt": {"revision": "b".repeat(40)}}),
                            )
                        }
                    } else if path.ends_with("/invitations") {
                        (
                            201,
                            json!({"id": "inv_1111111111111111", "developmentLink": "http://localhost/#synthetic-invitation-secret"}),
                        )
                    } else if path.ends_with("/repository/access")
                        && request.method() == &tiny_http::Method::Post
                    {
                        (
                            201,
                            json!({"id": "access_1111111111111111", "token": "synthetic-git-secret"}),
                        )
                    } else if path.ends_with("/brains")
                        && request.method() == &tiny_http::Method::Post
                    {
                        (
                            200,
                            json!({"organization": {"id": SECOND, "name": input["name"]}}),
                        )
                    } else if path.ends_with("/brains") {
                        if let Some(path) = state.select_during_discovery.take() {
                            fs::create_dir_all(path.parent().unwrap()).unwrap();
                            fs::write(path, format!("brain_id = '{SECOND}'\n")).unwrap();
                        }
                        if state.fail_discovery {
                            (503, json!({"error": "Cortex discovery unavailable"}))
                        } else {
                            (200, json!({"organizations": state.organizations}))
                        }
                    } else {
                        (200, json!({"completed": true}))
                    }
                } else {
                    (404, json!({"error": "not found"}))
                };
                let body = value.to_string().into_bytes();
                let body = match state.response_encoding {
                    Some("br") => {
                        let mut compressed = Vec::new();
                        {
                            let mut writer =
                                brotli::CompressorWriter::new(&mut compressed, 4096, 4, 22);
                            writer.write_all(&body).unwrap();
                        }
                        compressed
                    }
                    Some("gzip") => {
                        let mut writer = flate2::write::GzEncoder::new(
                            Vec::new(),
                            flate2::Compression::default(),
                        );
                        writer.write_all(&body).unwrap();
                        writer.finish().unwrap()
                    }
                    _ => body,
                };
                let mut response = tiny_http::Response::from_data(body)
                    .with_status_code(status)
                    .with_header(
                        tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap(),
                    );
                if let Some(encoding) = state.response_encoding {
                    response.add_header(
                        tiny_http::Header::from_bytes("Content-Encoding", encoding).unwrap(),
                    );
                }
                request.respond(response).unwrap();
            }
        });
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("project")).unwrap();
        Self {
            origin,
            root,
            state,
            stop,
            server: Some(worker),
        }
    }
    fn command(&self) -> Command {
        let mut command = self.local_command();
        command.args([
            "--project",
            self.root.path().join("project").to_str().unwrap(),
        ]);
        command
    }
    fn local_command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cortex"));
        command
            .args(["--origin", &self.origin])
            .current_dir(self.root.path().join("project"))
            .env("CORTEX_HOME", self.root.path().join("auth"))
            .env_remove("CORTEX_ORIGIN")
            .env("NO_COLOR", "1");
        command
    }
    fn run(&self, args: &[&str]) -> (i32, Value) {
        let output = self.command().args(args).output().unwrap();
        decode(output)
    }
    fn login(&self, args: &[&str]) -> Value {
        let mut child = self
            .command()
            .args(["login", "--no-browser", "--timeout", "10"])
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut reader = BufReader::new(child.stderr.take().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let event: Value = serde_json::from_str(&line).unwrap();
        let url = reqwest::Url::parse(event["url"].as_str().unwrap()).unwrap();
        let fields = url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        self.state.lock().unwrap().challenge = fields["code_challenge"].to_string();
        let mut callback = reqwest::Url::parse(&fields["redirect_uri"]).unwrap();
        callback
            .query_pairs_mut()
            .append_pair("code", "synthetic-code")
            .append_pair("state", "wrong-state")
            .append_pair("iss", &format!("{}/app/api/oauth", self.origin));
        assert_eq!(
            reqwest::blocking::get(callback.clone())
                .unwrap()
                .status()
                .as_u16(),
            400
        );
        callback.set_query(None);
        callback
            .query_pairs_mut()
            .append_pair("code", "synthetic-code")
            .append_pair("state", &fields["state"])
            .append_pair("iss", &format!("{}/app/api/oauth", self.origin));
        assert_eq!(
            reqwest::blocking::get(callback).unwrap().status().as_u16(),
            200
        );
        let output = child.wait_with_output().unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic-access-token"));
        let (code, result) = decode(output);
        assert_eq!(code, 0, "{result}");
        assert_eq!(result["data"]["signed_in"], true);
        result
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.server.take().unwrap().join().unwrap();
    }
}
fn decode(output: Output) -> (i32, Value) {
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "Unexpected CLI result: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap(), value)
}

#[test]
fn attachments_download_whole_files_and_verify_integrity_without_overwriting_local_work() {
    let fixture = Fixture::new();
    fixture.login(&[]);
    let output = fixture.root.path().join("template.docx");
    let path = "deliverables/_attachments/template.docx";
    let args = [
        "knowledge",
        "download",
        path,
        "--output",
        output.to_str().unwrap(),
    ];
    let (code, result) = fixture.run(&args);
    assert_eq!(code, 0, "{result}");
    assert_eq!(fs::read(&output).unwrap(), [0, 1, 255, 0]);
    assert!(result["data"].get("data").is_none());
    assert_eq!(
        fixture
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|(_, path, _)| path.ends_with("/knowledge/attachment"))
            .count(),
        1
    );
    assert_ne!(fixture.run(&args).0, 0);
    assert_eq!(fs::read(&output).unwrap(), [0, 1, 255, 0]);
    let corrupt = fixture.root.path().join("corrupt.bin");
    assert_ne!(
        fixture
            .run(&[
                "knowledge",
                "download",
                "finance/_attachments/corrupt.bin",
                "--output",
                corrupt.to_str().unwrap()
            ])
            .0,
        0
    );
    assert!(!corrupt.exists());
    let revision = "a".repeat(40);
    let (code, result) = fixture.run(&[
        "knowledge",
        "upload",
        path,
        "--file",
        output.to_str().unwrap(),
        "--base-revision",
        &revision,
        "--request-id",
        "upload-template",
        "--summary",
        "Save template",
    ]);
    assert_eq!(code, 0, "{result}");
    let state = fixture.state.lock().unwrap();
    let input = &state
        .requests
        .iter()
        .find(|(_, path, _)| path.ends_with("/knowledge/attachments"))
        .unwrap()
        .2;
    assert_eq!(input["attachment"]["data"], "AAH/AA==");
    assert_eq!(input["baseRevision"], revision);
    assert_eq!(input["mode"], "create");
}

#[test]
fn compressed_responses_support_search_login_refresh_and_service_errors() {
    for encoding in ["br", "gzip"] {
        let fixture = Fixture::new();
        fixture.login(&[]);
        fixture.state.lock().unwrap().response_encoding = Some(encoding);
        let (code, search) = fixture.run(&["search", "profit"]);
        assert_eq!(code, 0, "{encoding}: {search}");
        assert_eq!(search["data"]["results"][0]["snippet"], "Fixture policy");
        assert_eq!(fixture.state.lock().unwrap().refreshes, 1);
        fixture.state.lock().unwrap().deny_membership = true;
        let (code, denied) = fixture.run(&["whoami"]);
        assert_eq!(code, 4, "{encoding}: {denied}");
        assert_eq!(denied["error"]["status"], 403);
        assert_eq!(denied["error"]["code"], "permission_denied");
        assert_eq!(
            denied["error"]["message"],
            "Access changed during selection"
        );
        let login_fixture = Fixture::new();
        login_fixture.state.lock().unwrap().response_encoding = Some(encoding);
        let login = login_fixture.login(&[]);
        assert_eq!(login["data"]["selection"]["selected"], true);
        assert_eq!(login_fixture.state.lock().unwrap().refreshes, 1);
    }
}

#[test]
fn plain_commands_use_the_working_directory_selection_and_never_a_global_default() {
    let fixture = Fixture::new();
    fixture.login(&[]);
    let local = decode(fixture.local_command().arg("config").output().unwrap());
    assert_eq!(local.0, 0);
    assert_eq!(local.1["data"]["brain_id"], FIRST);
    let search = decode(
        fixture
            .local_command()
            .args(["search", "policy"])
            .output()
            .unwrap(),
    );
    assert_eq!(search.0, 0);
    assert_eq!(search.1["context"]["brain_id"], FIRST);
    let other = fixture.root.path().join("other");
    fs::create_dir_all(other.join(".cortex")).unwrap();
    fs::write(
        other.join(".cortex/config.toml"),
        format!("brain_id = '{SECOND}'\n"),
    )
    .unwrap();
    let search = decode(
        fixture
            .local_command()
            .current_dir(&other)
            .args(["search", "policy"])
            .output()
            .unwrap(),
    );
    assert_eq!(search.0, 0);
    assert_eq!(search.1["context"]["brain_id"], SECOND);
    let unconfigured = decode(
        fixture
            .local_command()
            .current_dir(fixture.root.path())
            .arg("config")
            .output()
            .unwrap(),
    );
    assert_eq!(unconfigured.0, 0);
    assert_eq!(unconfigured.1["data"]["brain_id"], Value::Null);
    assert_eq!(
        decode(
            fixture
                .local_command()
                .current_dir(fixture.root.path())
                .args(["search", "policy"])
                .output()
                .unwrap()
        )
        .0,
        2
    );
    assert!(!fixture.root.path().join(".cortex").exists());
    assert!(
        fs::read_dir(fixture.root.path().join("auth"))
            .unwrap()
            .all(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_none_or(|extension| extension != "toml"))
    );
}

#[test]
fn login_selects_the_only_cortex_and_preserves_existing_selection_on_relogin() {
    let fixture = Fixture::new();
    let result = fixture.login(&[]);
    assert_eq!(result["data"]["selection"]["selected"], true);
    assert_eq!(
        result["data"]["selection"]["identity"]["organization"]["id"],
        FIRST
    );
    assert_eq!(result["context"]["brain_id"], FIRST);
    assert_eq!(fixture.run(&["config"]).1["data"]["brain_id"], FIRST);
    assert_eq!(fixture.state.lock().unwrap().verification_count, 2);
    assert_eq!(fixture.run(&["search", "policy"]).0, 0);

    assert_eq!(fixture.run(&["use", SECOND]).0, 0);
    let path = fixture.root.path().join("project/.cortex/config.toml");
    let original = fs::read(&path).unwrap();
    fixture.state.lock().unwrap().requests.clear();
    let result = fixture.login(&[]);
    assert_eq!(result["data"]["selection"]["reason"], "already_selected");
    assert_eq!(result["context"]["brain_id"], SECOND);
    assert_eq!(fs::read(path).unwrap(), original);
    assert!(
        !fixture
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .any(|(_, path, _)| path.starts_with("/app/api/cli/"))
    );
}

#[test]
fn login_opt_out_and_invocation_override_leave_project_unconfigured() {
    for (args, reason, cortex) in [
        (vec!["--no-select"], "disabled", Value::Null),
        (vec!["--brain", SECOND], "brain_override", json!(SECOND)),
    ] {
        let fixture = Fixture::new();
        let result = fixture.login(&args);
        assert_eq!(result["data"]["selection"]["selected"], false);
        assert_eq!(result["data"]["selection"]["reason"], reason);
        assert_eq!(result["context"]["brain_id"], cortex);
        assert!(!fixture.root.path().join("project/.cortex").exists());
        assert!(
            !fixture
                .state
                .lock()
                .unwrap()
                .requests
                .iter()
                .any(|(_, path, _)| path.starts_with("/app/api/cli/"))
        );
        assert_eq!(fixture.run(&["brains", "list"]).0, 0);
    }
}

#[test]
fn login_requires_a_choice_with_zero_or_multiple_brains() {
    for (organizations, reason) in [
        (vec![], "no_brains"),
        (
            vec![json!({"id": FIRST}), json!({"id": SECOND})],
            "multiple_brains",
        ),
    ] {
        let fixture = Fixture::new();
        fixture.state.lock().unwrap().organizations = organizations;
        let result = fixture.login(&[]);
        assert_eq!(result["data"]["selection"]["selected"], false);
        assert_eq!(result["data"]["selection"]["reason"], reason);
        assert_eq!(result["context"]["brain_id"], Value::Null);
        assert!(!fixture.root.path().join("project/.cortex").exists());
        assert_eq!(fixture.state.lock().unwrap().verification_count, 0);
        assert_eq!(fixture.run(&["search", "policy"]).0, 2);
    }
}

#[test]
fn login_keeps_authentication_when_discovery_or_selection_fails() {
    for (discovery, denied, rollback, code, checks) in [
        (true, false, false, "service", 0),
        (false, true, false, "permission_denied", 1),
        (false, false, true, "permission_denied", 2),
    ] {
        let fixture = Fixture::new();
        {
            let mut state = fixture.state.lock().unwrap();
            state.fail_discovery = discovery;
            state.deny_membership = denied;
            state.fail_verification = rollback;
        }
        let result = fixture.login(&[]);
        assert_eq!(result["data"]["selection"]["selected"], false);
        assert_eq!(result["data"]["selection"]["reason"], "failed");
        assert_eq!(result["data"]["selection"]["error"]["code"], code);
        assert_eq!(result["context"]["brain_id"], Value::Null);
        assert!(
            !fixture
                .root
                .path()
                .join("project/.cortex/config.toml")
                .exists()
        );
        assert_eq!(fixture.state.lock().unwrap().verification_count, checks);
        fixture.state.lock().unwrap().fail_discovery = false;
        assert_eq!(fixture.run(&["brains", "list"]).0, 0);
    }
}

#[test]
fn login_preserves_legacy_or_invalid_project_configuration() {
    for (name, content) in [
        ("config.json", format!("{{\"brain_id\":\"{SECOND}\"}}")),
        ("config.toml", "brain_id = 'invalid'\n".into()),
    ] {
        let fixture = Fixture::new();
        let directory = fixture.root.path().join("project/.cortex");
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join(name), &content).unwrap();
        let result = fixture.login(&[]);
        assert_eq!(result["data"]["selection"]["selected"], false);
        assert_eq!(
            result["data"]["selection"]["reason"],
            if name == "config.json" {
                "already_selected"
            } else {
                "failed"
            }
        );
        assert_eq!(fs::read_to_string(directory.join(name)).unwrap(), content);
        if name == "config.json" {
            assert!(!directory.join("config.toml").exists());
        }
        assert!(
            !fixture
                .state
                .lock()
                .unwrap()
                .requests
                .iter()
                .any(|(_, path, _)| path.starts_with("/app/api/cli/"))
        );
    }
}

#[test]
fn login_preserves_a_selection_saved_during_cortex_discovery() {
    let fixture = Fixture::new();
    let path = fixture.root.path().join("project/.cortex/config.toml");
    fixture.state.lock().unwrap().select_during_discovery = Some(path.clone());
    let result = fixture.login(&[]);
    assert_eq!(result["data"]["selection"]["reason"], "already_selected");
    assert_eq!(result["context"]["brain_id"], SECOND);
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        format!("brain_id = '{SECOND}'\n")
    );
    assert_eq!(fixture.state.lock().unwrap().verification_count, 0);
}

#[test]
fn authenticated_workflow_preserves_selection_retry_identity_and_secrets() {
    let fixture = Fixture::new();
    fixture.login(&["--no-select"]);
    assert_eq!(fixture.run(&["search", "policy"]).0, 2);
    assert_eq!(fixture.run(&["use", FIRST]).0, 0);
    assert_eq!(fixture.state.lock().unwrap().refreshes, 1);
    let config_path = fixture.root.path().join("project/.cortex/config.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    assert!(config.contains(FIRST));
    assert!(!config.contains("token"));
    let search = fixture.run(&["--brain", SECOND, "search", "policy", "--limit", "3"]);
    assert_eq!(search.0, 0);
    assert_eq!(search.1["context"]["brain_id"], SECOND);
    assert_eq!(search.1["data"]["query"]["limit"], 3);
    assert!(
        search.1["data"]["results"][0]["source_url"]
            .as_str()
            .unwrap()
            .contains("revision=aaaaaaaa")
    );
    assert_eq!(fs::read_to_string(&config_path).unwrap(), config);
    let read = fixture.run(&["read", "policy.md", "--revision", &"a".repeat(40)]);
    assert_eq!(read.0, 0);
    assert_eq!(read.1["data"]["query"]["path"], "/policy.md");
    fixture.state.lock().unwrap().fail_verification = true;
    assert_eq!(fixture.run(&["use", SECOND]).0, 4);
    assert_eq!(fs::read_to_string(&config_path).unwrap(), config);
    fixture.state.lock().unwrap().fail_verification = false;
    let created = fixture.run(&[
        "brains",
        "create",
        "New Cortex",
        "--request-id",
        "stable-creation",
    ]);
    assert_eq!(created.0, 0);
    assert_eq!(fs::read_to_string(&config_path).unwrap(), config);
    let recorded = fixture.run(&[
        "record",
        "--text",
        "Remember the fixture",
        "--request-id",
        "stable-recording",
    ]);
    assert_eq!(recorded.0, 0);
    assert_eq!(recorded.1["data"]["status"], "saved");
    fixture.state.lock().unwrap().wait_fails = true;
    let uncertain = fixture.run(&[
        "record",
        "--text",
        "Same facts",
        "--request-id",
        "uncertain-recording",
    ]);
    assert_eq!(uncertain.0, 1);
    assert_eq!(
        uncertain.1["error"]["details"]["context"]["request_id"],
        "uncertain-recording"
    );
    assert_eq!(
        uncertain.1["error"]["details"]["operation"]["submission"]["runId"],
        RUN
    );
    let private = fixture.root.path().join("git.json");
    let access = fixture.run(&[
        "repository",
        "access",
        "create",
        "fixture",
        "--secret-file",
        private.to_str().unwrap(),
    ]);
    assert_eq!(access.0, 0);
    assert!(!access.1.to_string().contains("synthetic-git-secret"));
    assert!(
        fs::read_to_string(&private)
            .unwrap()
            .contains("synthetic-git-secret")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&private).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_ne!(
        fixture
            .run(&[
                "repository",
                "access",
                "create",
                "fixture",
                "--secret-file",
                private.to_str().unwrap()
            ])
            .0,
        0
    );
    let invitation = fixture.root.path().join("invitation.json");
    let invited = fixture.run(&[
        "people",
        "invite",
        "friend@example.com",
        "--role",
        "read",
        "--request-id",
        "invite-fixture",
        "--secret-file",
        invitation.to_str().unwrap(),
    ]);
    assert_eq!(invited.0, 0);
    assert!(
        !invited
            .1
            .to_string()
            .contains("synthetic-invitation-secret")
    );
    assert!(
        fs::read_to_string(invitation)
            .unwrap()
            .contains("synthetic-invitation-secret")
    );
    let state = fixture.state.lock().unwrap();
    let creation = state
        .requests
        .iter()
        .find(|(_, path, _)| path.ends_with("/brains"))
        .unwrap();
    assert_eq!(creation.2["brain_id"], FIRST);
    assert_eq!(creation.2["requestId"], "stable-creation");
    assert_eq!(
        state
            .requests
            .iter()
            .filter(|(_, path, _)| path.ends_with("/repository/access"))
            .count(),
        1
    );
    drop(state);
    assert_eq!(fixture.run(&["logout"]).0, 0);
    assert!(fixture.state.lock().unwrap().revoked);
    assert_eq!(fixture.run(&["account", "show"]).0, 3);
}

#[test]
fn parsing_config_and_stdin_are_predictable_without_authentication() {
    let fixture = Fixture::new();
    let help = fixture
        .command()
        .args(["people", "invite", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--role"));
    let schema = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .arg("__usage_spec__")
        .output()
        .unwrap();
    assert!(schema.status.success());
    assert!(String::from_utf8_lossy(&schema.stdout).contains("knowledge"));
    let completions = fixture
        .command()
        .args(["completions", "zsh"])
        .output()
        .unwrap();
    assert!(completions.status.success());
    assert!(String::from_utf8_lossy(&completions.stdout).contains("cortex"));
    assert_eq!(fixture.run(&["unknown-command"]).0, 2);
    assert_eq!(fixture.run(&["record", "--text", "facts"]).0, 2);
    assert_eq!(fixture.run(&["config"]).1["data"]["brain_id"], Value::Null);
    let directory = fixture.root.path().join("project/.cortex");
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("config.json"),
        format!("{{\"brain_id\":\"{FIRST}\"}}"),
    )
    .unwrap();
    assert_eq!(fixture.run(&["config"]).0, 0);
    fs::write(directory.join("config.toml"), "brain_id = 'invalid'\n").unwrap();
    assert_eq!(fixture.run(&["config"]).0, 2);
    assert_eq!(fixture.run(&["--brain", FIRST, "config"]).0, 0);
    assert_eq!(
        fixture
            .run(&["--origin", "http://example.com", "request-id"])
            .0,
        2
    );
    fixture.login(&["--no-select"]);
    assert_eq!(fixture.run(&["use", FIRST]).0, 0);
    let mut child = fixture
        .command()
        .args([
            "record",
            "--file",
            "-",
            "--request-id",
            "stdin-test",
            "--wait",
            "0",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"Facts from stdin")
        .unwrap();
    assert_eq!(decode(child.wait_with_output().unwrap()).0, 0);
    assert_eq!(
        fixture
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .find(|(_, path, _)| path.ends_with("/knowledge/record"))
            .unwrap()
            .2["info"],
        "Facts from stdin"
    );
}
