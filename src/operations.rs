use crate::{
    api::Api,
    args::*,
    auth, config,
    error::{Error, Result},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::Method;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub struct Context {
    pub api: Api,
    pub project: PathBuf,
    pub selected: Option<String>,
}

impl Context {
    fn brain(&self) -> Result<&str> {
        self.selected.as_deref().ok_or_else(|| {
            Error::new(
                "config",
                "No Brain selected. Choose one with cortex brains list and cortex use <id>.",
            )
        })
    }
    fn get(&self, path: &str) -> Result<Value> {
        self.api.org(self.brain()?, Method::GET, path, &[], None)
    }
    fn mutate(&self, method: Method, path: &str, input: Value) -> Result<Value> {
        self.api.org(self.brain()?, method, path, &[], Some(&input))
    }
    fn query(&self, input: Value, revision: Option<&str>) -> Result<Value> {
        let mut query = revision_query(revision)?;
        query.push(("input", input.to_string()));
        let mut result = self.api.org(
            self.brain()?,
            Method::GET,
            "/knowledge/retrieve",
            &query,
            None,
        )?;
        add_sources(&mut result, &self.api.origin, self.brain()?);
        Ok(result)
    }
    pub fn execute(&mut self, command: Command) -> Result<Value> {
        match command {
            Command::Completions(_) => Err(Error::invalid("Completions are handled before service operations")),
            Command::Login(options) => {
                let mut result = auth::login(&self.api.origin, &options)?;
                result["selection"] = if options.no_select {
                    json!({"selected": false, "reason": "disabled"})
                } else if self.selected.is_some() {
                    json!({"selected": false, "reason": "brain_override"})
                } else {
                    self.select_only_brain().unwrap_or_else(|error| {
                        json!({"selected": false, "reason": "failed", "error": error})
                    })
                };
                Ok(result)
            },
            Command::Logout => auth::logout(&self.api.origin),
            Command::RequestId => Ok(json!({"request_id": uuid::Uuid::new_v4().to_string()})),
            Command::Config => Ok(json!({"project": self.project, "brain_id": self.selected, "origin": self.api.origin})),
            Command::RecordingAllowance => self.get("/recording-allowance"),
            Command::Account(options) => match options.command {
                AccountCommand::Show => self.api.get("/account"),
                AccountCommand::Profile(options) => self.api.request(Method::PATCH, "/profile", &[], Some(&json!({"name": options.name}))),
                AccountCommand::Disconnect(options) => self.api.request(Method::DELETE, &format!("/connections/{}", config::identifier(&options.connection_id, "ogr")?), &[], None),
            },
            Command::Brains(options) => match options.command {
                BrainCommand::List => self.api.get("/brains"),
                BrainCommand::Create(options) => {
                    request_id(&options.request_id)?;
                    let mut input = json!({"name": options.name, "requestId": options.request_id});
                    if let Some(brain) = &self.selected { input["brain_id"] = json!(brain); }
                    self.api.request(Method::POST, "/brains", &[], Some(&input))
                }
            },
            Command::Use(options) => self.select(&options.brain_id, false),
            Command::Whoami => self.api.whoami(self.brain()?),
            Command::Status => self.get(""),
            Command::Search(options) => self.search(options),
            Command::Read(options) => self.read(options),
            Command::Record(options) => self.record(options),
            Command::Knowledge(options) => self.knowledge(options.command),
            Command::Access(options) => match options.command {
                KnowledgeAccessCommand::Show(options) => match options.user_id { Some(user) => self.get(&format!("/access/members/{}", config::identifier(&user, "usr")?)), None => self.get("/access") },
                KnowledgeAccessCommand::Explain(options) => {
                    let mut input = json!({"path": options.path});
                    if let Some(user) = options.user_id { input["userId"] = json!(config::identifier(&user, "usr")?); }
                    self.mutate(Method::POST, "/access/explain", input)
                },
                KnowledgeAccessCommand::Scopes(options) => match options.file { Some(file) => self.mutate(Method::POST, "/access/scopes", json_input(&file)?), None => self.get("/access/scopes") },
                KnowledgeAccessCommand::Operations => self.get("/access/operations"),
                KnowledgeAccessCommand::Activate(options) => match options.file { Some(file) => self.mutate(Method::POST, "/access/activation", json_input(&file)?), None => self.get("/access/activation") },
                KnowledgeAccessCommand::Promote(options) => self.mutate(Method::POST, "/access/promote", json_input(&options.file)?),
            },
            Command::Runs(options) => match options.command {
                RunCommand::Get(options) => self.wait(&options.run_id, options.wait),
                RunCommand::Cancel(options) => self.mutate(Method::POST, &format!("/runs/{}/cancel", config::identifier(&options.run_id, "run")?), json!({})),
            },
            Command::People(options) => match options.command {
                PeopleCommand::List => self.get("/people"),
                PeopleCommand::Invite(options) => {
                    request_id(&options.request_id)?;
                    let access = match (options.role, options.policy_file) {
                        (Some(role), None) => json!({"kind": role}),
                        (None, Some(file)) => json_input(&file)?,
                        _ => return Err(Error::invalid("Choose exactly one of --role read|write|admin or --policy-file; permissions are required")),
                    };
                    let mut secret = options.secret_file.as_deref().map(|path| self.secret_file(path)).transpose()?;
                    let response = self.mutate(Method::POST, "/invitations", json!({"email": options.email, "access": access, "requestId": options.request_id}));
                    let mut result = match response { Ok(result) => result, Err(error) => {
                        if let Some(path) = &options.secret_file { let _ = fs::remove_file(path); }
                        return Err(error);
                    }};
                    if let Some(file) = &mut secret { file.write_all(&serde_json::to_vec(&result)?)?; file.sync_all()?; }
                    if let Some(object) = result.as_object_mut() && object.remove("developmentLink").is_some() {
                        object.insert("development_link".into(), json!(if secret.is_some() { "saved_to_secret_file" } else { "discarded; request a new invitation with --secret-file for local acceptance" }));
                    }
                    if let Some(path) = options.secret_file { result["secret_file"] = json!(path); }
                    Ok(result)
                },
                PeopleCommand::Revoke(options) => self.mutate(Method::DELETE, &format!("/invitations/{}", config::identifier(&options.invitation_id, "inv")?), json!({})),
                PeopleCommand::Role(options) => self.mutate(Method::PATCH, &format!("/people/{}", config::identifier(&options.user_id, "usr")?), json!({"role": options.role})),
                PeopleCommand::Transfer(options) => self.mutate(Method::PUT, "/ownership", json!({"userId": config::identifier(&options.user_id, "usr")?, "expectedOwnerId": config::identifier(&options.expected_owner, "usr")?})),
                PeopleCommand::Access(options) => {
                    let input = json_input(&options.file)?;
                    if options.preview { self.mutate(Method::POST, "/access/preview", input) }
                    else {
                        let user = input.get("userId").and_then(Value::as_str).ok_or_else(|| Error::invalid("Policy input requires userId"))?;
                        self.mutate(Method::PUT, &format!("/access/members/{}", config::identifier(user, "usr")?), input)
                    }
                },
            },
            Command::Connections(options) => match options.command {
                ConnectionCommand::List => self.get("/connections"),
                ConnectionCommand::Disconnect(options) => self.mutate(Method::DELETE, &format!("/connections/{}", config::identifier(&options.connection_id, "ogr")?), json!({})),
            },
            Command::Repository(options) => match options.command {
                RepositoryCommand::Retry => self.mutate(Method::POST, "/repository/retry", json!({})),
                RepositoryCommand::Access(options) => match options.command {
                    AccessCommand::List => self.get("/repository/access"),
                    AccessCommand::Revoke(options) => self.mutate(Method::DELETE, &format!("/repository/access/{}", config::identifier(&options.access_id, "access")?), json!({})),
                    AccessCommand::Create(options) => {
                        let path = Path::new(&options.secret_file);
                        let mut file = self.secret_file(&options.secret_file)?;
                        let result = self.mutate(Method::POST, "/repository/access", json!({"name": options.name}));
                        match result {
                            Ok(value) => {
                                file.write_all(&serde_json::to_vec(&value)?)?;
                                file.sync_all()?;
                                Ok(json!({"created": true, "secret_file": path, "access_id": value.get("id")}))
                            },
                            Err(error) => { let _ = fs::remove_file(path); Err(error) }
                        }
                    }
                }
            },
        }
    }

    fn secret_file(&self, value: &str) -> Result<fs::File> {
        let path = Path::new(value);
        if !path.is_absolute()
            || path
                .parent()
                .ok_or_else(|| Error::invalid("Invalid secret file destination"))?
                .canonicalize()?
                .starts_with(&self.project)
        {
            return Err(Error::invalid(
                "Choose an absolute secret-file path outside the project, in an existing private directory",
            ));
        }
        config::private_file(path)
    }

    fn existing_selection(&mut self) -> Result<Option<Value>> {
        let Some(id) = config::selected(&self.project)? else {
            return Ok(None);
        };
        self.selected = Some(id.clone());
        Ok(Some(
            json!({"selected": false, "reason": "already_selected", "brain_id": id}),
        ))
    }

    fn select_only_brain(&mut self) -> Result<Value> {
        if let Some(selection) = self.existing_selection()? {
            return Ok(selection);
        }
        let account = self.api.get("/brains")?;
        let brains = account
            .get("organizations")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::new("protocol", "Expected a list of Brains"))?;
        if brains.len() != 1 {
            return Ok(
                json!({"selected": false, "reason": if brains.is_empty() { "no_brains" } else { "multiple_brains" }}),
            );
        }
        let id = brains[0]
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("protocol", "Expected a Brain ID"))?;
        self.select(id, true)
    }

    fn select(&mut self, id: &str, only_if_unset: bool) -> Result<Value> {
        config::identifier(id, "org")?;
        let directory = self.project.join(".cortex");
        config::reject_symlink(&directory)?;
        fs::create_dir_all(&directory)?;
        let _guard = config::lock(&directory.join("config.lock"))?;
        let path = directory.join("config.toml");
        config::reject_symlink(&path)?;
        if only_if_unset && let Some(selection) = self.existing_selection()? {
            return Ok(selection);
        }
        let previous = if path.exists() {
            Some(fs::read(&path)?)
        } else {
            None
        };
        self.api.whoami(id)?;
        let content = toml::to_string(&config::Selection {
            brain_id: id.into(),
        })
        .map_err(|_| Error::new("config", "Could not encode Brain selection"))?;
        config::atomic_write(&path, content.as_bytes())?;
        let verified = (|| {
            if config::selected(&self.project)?.as_deref() != Some(id) {
                return Err(Error::new("config", "Brain selection readback mismatch"));
            }
            self.api.whoami(id)
        })();
        match verified {
            Ok(identity) => {
                self.selected = Some(id.into());
                Ok(json!({"selected": true, "identity": identity, "config": path}))
            }
            Err(error) => {
                if let Some(previous) = previous {
                    config::atomic_write(&path, &previous)?;
                } else {
                    fs::remove_file(&path)?;
                }
                Err(error)
            }
        }
    }

    fn knowledge(&self, command: KnowledgeCommand) -> Result<Value> {
        match command {
            KnowledgeCommand::Copy(options) => {
                self.mutate(Method::POST, "/knowledge/copy", json_input(&options.file)?)
            }
            KnowledgeCommand::Upload(options) => {
                request_id(&options.request_id)?;
                revision_query(Some(&options.base_revision))?;
                let bytes = config::read_input(&options.file, 5 * 1024 * 1024)?;
                let name = Path::new(&options.file)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| Error::invalid("Invalid attachment filename"))?;
                let mut input = json!({"path": options.path, "attachment": {"name": name, "data": STANDARD.encode(bytes)}, "mode": if options.replace {"replace"} else {"create"}, "baseRevision": options.base_revision, "requestId": options.request_id, "summary": options.summary});
                if let Some(sources) = options.sources {
                    input["sources"] = json_input(&sources)?;
                }
                self.mutate(Method::POST, "/knowledge/attachments", input)
            }
            KnowledgeCommand::Download(options) => {
                let mut query = revision_query(options.revision.as_deref())?;
                query.extend([("path", options.path), ("download", "base64".into())]);
                let mut result = self.api.org(
                    self.brain()?,
                    Method::GET,
                    "/knowledge/attachment",
                    &query,
                    None,
                )?;
                let encoded = result
                    .get("data")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::new("protocol", "Download did not contain file bytes"))?;
                if encoded.len() > 4 * (5 * 1024 * 1024_usize).div_ceil(3) {
                    return Err(Error::new("protocol", "Download exceeds 5 MiB"));
                }
                let bytes = STANDARD
                    .decode(encoded)
                    .map_err(|_| Error::new("protocol", "Invalid file encoding"))?;
                let checksum = format!("{:x}", Sha256::digest(&bytes));
                if result.get("sha256").and_then(Value::as_str) != Some(checksum.as_str())
                    || result.get("byteCount").and_then(Value::as_u64) != Some(bytes.len() as u64)
                {
                    return Err(Error::new("protocol", "Download integrity check failed"));
                }
                let mut output = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&options.output)?;
                if let Err(error) = output.write_all(&bytes).and_then(|()| output.sync_all()) {
                    drop(output);
                    let _ = fs::remove_file(&options.output);
                    return Err(error.into());
                }
                result
                    .as_object_mut()
                    .ok_or_else(|| Error::new("protocol", "Invalid download result"))?
                    .remove("data");
                result["output"] = json!(options.output);
                Ok(result)
            }
            KnowledgeCommand::Patch(options) => {
                self.mutate(Method::PATCH, "/knowledge", json_input(&options.file)?)
            }
            KnowledgeCommand::DiscardMove(options) => self.mutate(
                Method::POST,
                "/access/moves/discard",
                json_input(&options.file)?,
            ),
            KnowledgeCommand::Move(options) => self.mutate(
                Method::POST,
                if options.preview {
                    "/access/moves/plan"
                } else {
                    "/access/moves/apply"
                },
                json_input(&options.file)?,
            ),
            KnowledgeCommand::Show => self.get("/knowledge"),
            KnowledgeCommand::Document(options) => {
                let mut query = revision_query(options.revision.as_deref())?;
                query.push((
                    "path",
                    knowledge_path(&options.path, false)?
                        .trim_start_matches('/')
                        .into(),
                ));
                let mut result =
                    self.api
                        .org(self.brain()?, Method::GET, "/knowledge", &query, None)?;
                result["path"] = json!(options.path);
                add_sources(&mut result, &self.api.origin, self.brain()?);
                Ok(result)
            }
            KnowledgeCommand::List(options) => self.api.org(
                self.brain()?,
                Method::GET,
                "/knowledge/files",
                &revision_query(options.revision.as_deref())?,
                None,
            ),
            KnowledgeCommand::Revision => self.get("/knowledge/revision"),
            KnowledgeCommand::Search(options) => self.search(options),
            KnowledgeCommand::Grep(options) => {
                pagination(options.offset, options.limit)?;
                let mut input = json!({"op": "grep", "pattern": options.pattern, "ignoreCase": !options.case_sensitive, "offset": options.offset, "limit": options.limit});
                if let Some(cursor) = options.cursor {
                    input["cursor"] = json!(cursor);
                }
                if let Some(path) = options.path {
                    input["path"] = json!(knowledge_path(&path, true)?);
                }
                self.query(input, options.revision.as_deref())
            }
            KnowledgeCommand::Read(options) => self.read(options),
            KnowledgeCommand::ReadMany(options) => {
                let mut files: Value =
                    serde_json::from_slice(&config::read_input(&options.file, 8192)?)?;
                let ranges = files
                    .as_array_mut()
                    .ok_or_else(|| Error::invalid("Expected a JSON array of file ranges"))?;
                if ranges.is_empty() || ranges.len() > 8 {
                    return Err(Error::invalid("Supply 1-8 file ranges"));
                }
                for range in ranges {
                    range["path"] = json!(knowledge_path(
                        range
                            .get("path")
                            .and_then(Value::as_str)
                            .ok_or_else(|| Error::invalid("Each range needs a path"))?,
                        false
                    )?);
                }
                self.query(
                    json!({"op": "readMany", "files": files}),
                    options.revision.as_deref(),
                )
            }
            KnowledgeCommand::Record(options) => self.record(options),
            KnowledgeCommand::Replace(options) => {
                let input: Value =
                    serde_json::from_slice(&config::read_input(&options.file, 98304)?)?;
                let object = input
                    .as_object()
                    .ok_or_else(|| Error::invalid("Expected a JSON object"))?;
                if object.keys().any(|key| {
                    !["content", "baseRevision", "documents", "summary"].contains(&key.as_str())
                }) || !input["content"].is_string()
                {
                    return Err(Error::invalid(
                        "Expected content, baseRevision, and optional documents and summary",
                    ));
                }
                revision_query(Some(
                    input["baseRevision"]
                        .as_str()
                        .ok_or_else(|| Error::invalid("baseRevision is required"))?,
                ))?;
                self.mutate(Method::PUT, "/knowledge", input)
            }
            KnowledgeCommand::Attachment(options) => {
                bounded(options.start_line, u64::MAX, "start-line")?;
                bounded(options.limit, 200, "limit")?;
                let mut query = revision_query(options.revision.as_deref())?;
                query.extend([
                    ("path", options.path),
                    ("startLine", options.start_line.to_string()),
                    ("limit", options.limit.to_string()),
                ]);
                self.api.org(
                    self.brain()?,
                    Method::GET,
                    "/knowledge/attachment",
                    &query,
                    None,
                )
            }
        }
    }

    fn search(&self, options: Search) -> Result<Value> {
        pagination(options.offset, options.limit)?;
        let mut input = json!({"op": "search", "query": options.query, "offset": options.offset, "limit": options.limit});
        if let Some(cursor) = options.cursor {
            input["cursor"] = json!(cursor);
        }
        if let Some(path) = options.path {
            input["path"] = json!(knowledge_path(&path, true)?);
        }
        self.query(input, options.revision.as_deref())
    }
    fn read(&self, options: Read) -> Result<Value> {
        bounded(options.start_line, u64::MAX, "start-line")?;
        bounded(options.limit, 200, "limit")?;
        self.query(json!({"op": "read", "path": knowledge_path(&options.path, false)?, "startLine": options.start_line, "limit": options.limit}), options.revision.as_deref())
    }
    fn wait(&self, run: &str, seconds: u64) -> Result<Value> {
        if seconds > 25 {
            return Err(Error::invalid("wait must be 0-25 seconds"));
        }
        self.api.org(
            self.brain()?,
            Method::GET,
            &format!("/runs/{}", config::identifier(run, "run")?),
            &[("waitMs", (seconds * 1000).to_string())],
            None,
        )
    }
    fn record(&self, options: Record) -> Result<Value> {
        request_id(&options.request_id)?;
        if options.wait > 25 {
            return Err(Error::invalid("wait must be 0-25 seconds"));
        }
        let info = match (options.text, options.file) {
            (Some(text), None) => text,
            (None, Some(path)) => String::from_utf8(config::read_input(&path, 60000)?)
                .map_err(|_| Error::invalid("Facts must be UTF-8 text"))?,
            _ => {
                return Err(Error::invalid(
                    "Supply exactly one of --text or --file; use --file - for stdin",
                ));
            }
        };
        if info.trim().is_empty() || info.chars().count() > 15000 {
            return Err(Error::invalid("Facts must contain 1-15000 characters"));
        }
        if options.attachment.len() > 5 {
            return Err(Error::invalid("At most five attachments are accepted"));
        }
        let mut attachments = Vec::new();
        let mut total = 0;
        for path in options.attachment {
            let bytes = config::read_input(&path, 5 * 1024 * 1024)?;
            total += bytes.len();
            if total > 10 * 1024 * 1024 {
                return Err(Error::invalid("Attachments exceed 10 MiB total"));
            }
            let name = Path::new(&path)
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| Error::invalid("Invalid attachment filename"))?;
            attachments.push(json!({"name": name, "data": STANDARD.encode(bytes)}));
        }
        let mut input =
            json!({"info": info, "requestId": options.request_id, "attachments": attachments});
        if let Some(sources) = options.sources {
            input["sources"] = json_input(&sources)?;
        }
        if let Some(target) = options.target {
            input["target"] = json!(target);
        }
        let mut result = self.mutate(Method::POST, "/knowledge/record", input)?;
        if options.wait > 0
            && let Some(run_id) = result.get("runId").and_then(Value::as_str)
        {
            result = self.wait(run_id, options.wait).map_err(|mut error| {
                error.details = Some(json!({"submission": result}));
                error
            })?;
        }
        Ok(result)
    }
}

fn request_id(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 80 || value.chars().any(char::is_control) {
        return Err(Error::invalid(
            "request-id must contain 1-80 characters without control characters",
        ));
    }
    Ok(())
}
fn bounded(value: u64, max: u64, name: &str) -> Result<()> {
    if value == 0 || value > max {
        return Err(Error::invalid(format!("{name} must be 1-{max}")));
    }
    Ok(())
}
fn pagination(offset: u64, limit: u64) -> Result<()> {
    bounded(limit, 200, "limit")?;
    if offset > 1000000 {
        return Err(Error::invalid("offset must be at most 1000000"));
    }
    Ok(())
}
fn json_input(file: &str) -> Result<Value> {
    Ok(serde_json::from_slice(&config::read_input(file, 98304)?)?)
}

fn knowledge_path(value: &str, directory: bool) -> Result<String> {
    if value == "/" && directory {
        return Ok(value.into());
    }
    let path = value.strip_prefix('/').unwrap_or(value);
    if path.is_empty()
        || path.len() > 240
        || path.split('/').any(|part| {
            !part
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
                || !part
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        })
    {
        return Err(Error::invalid(
            "Expected a repository-relative knowledge path without traversal",
        ));
    }
    Ok(format!("/{path}"))
}
fn revision_query(revision: Option<&str>) -> Result<Vec<(&'static str, String)>> {
    match revision {
        None => Ok(vec![]),
        Some(value)
            if value.len() == 40
                && value
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) =>
        {
            Ok(vec![("revision", value.into())])
        }
        _ => Err(Error::invalid(
            "revision must be a 40-character lowercase Git SHA",
        )),
    }
}
fn add_sources(value: &mut Value, origin: &str, brain: &str) {
    let Some(revision) = value
        .get("revision")
        .and_then(Value::as_str)
        .map(String::from)
    else {
        return;
    };
    fn visit(value: &mut Value, origin: &str, brain: &str, revision: &str) {
        match value {
            Value::Object(object) => {
                if let Some(path) = object.get("path").and_then(Value::as_str) {
                    let mut url =
                        reqwest::Url::parse(&format!("{origin}/app/{brain}/knowledge")).unwrap();
                    url.query_pairs_mut()
                        .append_pair("path", path.trim_start_matches('/'))
                        .append_pair("revision", revision);
                    object.insert("source_url".into(), json!(url.as_str()));
                }
                for (_, child) in object.iter_mut() {
                    visit(child, origin, brain, revision);
                }
            }
            Value::Array(values) => {
                for child in values {
                    visit(child, origin, brain, revision);
                }
            }
            _ => {}
        }
    }
    visit(value, origin, brain, &revision);
}
