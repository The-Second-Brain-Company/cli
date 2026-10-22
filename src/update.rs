use crate::{
    config,
    error::{Error, Result},
};
use reqwest::blocking::{Client, Response};
use semver::Version;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const RELEASES: &str = "https://github.com/The-Second-Brain-Company/cli/releases";
const MAX_BINARY_BYTES: u64 = 128 * 1024 * 1024;

struct Releases {
    client: Client,
    root: String,
}

struct Temporary(PathBuf);

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl Releases {
    fn official() -> Result<Self> {
        let client = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 || attempt.url().scheme() != "https" {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .timeout(Duration::from_secs(90))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("cortex-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            client,
            root: RELEASES.into(),
        })
    }

    fn response(&self, path: &str) -> Result<Response> {
        let response = self.client.get(format!("{}{path}", self.root)).send()?;
        if !response.status().is_success() {
            return Err(Error::new(
                "update",
                "Could not download the official CLI release. Retry cortex update when the release is available.",
            ));
        }
        Ok(response)
    }

    fn text(&self, path: &str) -> Result<String> {
        let mut bytes = Vec::new();
        self.response(path)?.take(1025).read_to_end(&mut bytes)?;
        if bytes.len() > 1024 {
            return Err(Error::new(
                "update",
                "Official release metadata exceeds its byte limit",
            ));
        }
        String::from_utf8(bytes)
            .map_err(|_| Error::new("update", "Official release metadata is not valid text"))
    }

    fn install(&self, path: &Path, platform: &str) -> Result<Value> {
        let directory = path.parent().ok_or_else(install_error)?;
        let name = path.file_name().ok_or_else(install_error)?;
        let lock = directory.join(format!(".{}.update.lock", name.to_string_lossy()));
        let _guard = config::lock(&lock).map_err(|error| {
            if error.code == "busy" {
                error
            } else {
                install_error()
            }
        })?;
        config::reject_symlink(path)?;
        let current = executable_version(path)?;
        let version = stable_version(&self.text("/latest/download/latest.txt")?)?;
        if !version.cmp_precedence(&current).is_gt() {
            return Ok(
                json!({"status": "current", "previousVersion": current.to_string(), "version": current.to_string(), "path": path}),
            );
        }
        let asset = format!("cortex-{platform}");
        let release_path = format!("/download/v{version}/{asset}");
        let checksum = self.text(&format!("{release_path}.sha256"))?;
        let expected = checksum_record(&checksum, &asset)?;
        let temporary =
            Temporary(directory.join(format!(".cortex-update-{}.tmp", uuid::Uuid::new_v4())));
        let mut file = config::private_file(&temporary.0).map_err(|_| install_error())?;
        let response = self.response(&release_path)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BINARY_BYTES)
        {
            return Err(Error::new(
                "update",
                "Official CLI download exceeds its byte limit",
            ));
        }
        let mut reader = response.take(MAX_BINARY_BYTES + 1);
        let mut digest = Sha256::new();
        let mut count = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            count += read as u64;
            if count > MAX_BINARY_BYTES {
                return Err(Error::new(
                    "update",
                    "Official CLI download exceeds its byte limit",
                ));
            }
            digest.update(&buffer[..read]);
            file.write_all(&buffer[..read])
                .map_err(|_| install_error())?;
        }
        if format!("{:x}", digest.finalize()) != expected {
            return Err(Error::new(
                "update",
                "Official CLI checksum verification failed; the installed executable was preserved",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o755))
                .map_err(|_| install_error())?;
        }
        file.sync_all().map_err(|_| install_error())?;
        drop(file);
        if executable_version(&temporary.0)? != version {
            return Err(Error::new(
                "update",
                "The downloaded CLI does not match the release version; the installed executable was preserved",
            ));
        }
        config::reject_symlink(path)?;
        fs::rename(&temporary.0, path).map_err(|_| install_error())?;
        Ok(
            json!({"status": "updated", "previousVersion": current.to_string(), "version": version.to_string(), "path": path}),
        )
    }
}

fn install_error() -> Error {
    Error::new(
        "update",
        "Could not replace the installed Cortex executable. Ensure its installation directory is writable, or rerun the installer in a writable directory.",
    )
}

fn stable_version(value: &str) -> Result<Version> {
    Version::parse(value.trim())
        .ok()
        .filter(|version| version.pre.is_empty())
        .ok_or_else(|| {
            Error::new(
                "update",
                "The official release must identify a valid stable semantic version",
            )
        })
}

fn checksum_record(value: &str, asset: &str) -> Result<String> {
    let record = value.strip_suffix('\n').unwrap_or(value);
    let (digest, name) = record
        .split_once("  ")
        .ok_or_else(|| Error::new("update", "The official checksum record is invalid"))?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) || name != asset {
        return Err(Error::new(
            "update",
            "The official checksum record does not identify the downloaded CLI",
        ));
    }
    Ok(digest.to_ascii_lowercase())
}

fn executable_version(path: &Path) -> Result<Version> {
    let output = Command::new(path).arg("--version").output().map_err(|_| Error::new("update", "Could not verify the Cortex executable version; the installed executable was preserved"))?;
    if !output.status.success() || !output.stderr.is_empty() {
        return Err(Error::new(
            "update",
            "The executable did not identify itself as Cortex; the installed executable was preserved",
        ));
    }
    let text = std::str::from_utf8(&output.stdout).map_err(|_| {
        Error::new(
            "update",
            "The executable returned an invalid Cortex version",
        )
    })?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    let version = text.strip_prefix("cortex ").and_then(|text| Version::parse(text).ok()).ok_or_else(|| Error::new("update", "The executable did not identify itself as Cortex; the installed executable was preserved"))?;
    if text != format!("cortex {version}") {
        return Err(Error::new(
            "update",
            "The executable returned an invalid Cortex version",
        ));
    }
    Ok(version)
}

pub fn run() -> Result<Value> {
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => {
            return Err(Error::new(
                "update",
                "No official CLI release supports this platform. Use the Cortex plugin MCP connection.",
            ));
        }
    };
    let path = std::env::current_exe()?.canonicalize()?;
    Releases::official()?.install(&path, platform)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        os::unix::fs::PermissionsExt,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        thread,
    };
    use tempfile::TempDir;

    const PLATFORM: &str = "aarch64-apple-darwin";
    type ReleaseResponses = HashMap<String, (u16, Vec<u8>)>;

    struct Fixture {
        root: TempDir,
        path: PathBuf,
        releases: Releases,
        responses: Arc<Mutex<ReleaseResponses>>,
        requests: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
        original: Vec<u8>,
    }

    fn executable(version: &str) -> Vec<u8> {
        format!("#!/bin/sh\nprintf 'cortex {version}\\n'\n").into_bytes()
    }

    impl Fixture {
        fn new(current: &str, latest: &str) -> Self {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("cortex");
            let original = executable(current);
            fs::write(&path, &original).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            fs::create_dir(root.path().join(".cortex")).unwrap();
            fs::write(
                root.path().join(".cortex/config.toml"),
                "brain_id = 'org_1111111111111111'\n",
            )
            .unwrap();
            fs::write(
                root.path().join("credentials.json"),
                "synthetic-private-credentials",
            )
            .unwrap();
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let origin = format!(
                "http://127.0.0.1:{}",
                server.server_addr().to_ip().unwrap().port()
            );
            let binary = executable(latest);
            let asset = format!("cortex-{PLATFORM}");
            let release_path = format!("/download/v{latest}/{asset}");
            let responses = Arc::new(Mutex::new(HashMap::from([
                (
                    "/latest/download/latest.txt".into(),
                    (200, format!("{latest}\n").into_bytes()),
                ),
                (release_path.clone(), (200, binary.clone())),
                (
                    format!("{release_path}.sha256"),
                    (
                        200,
                        format!("{:x}  {asset}\n", Sha256::digest(&binary)).into_bytes(),
                    ),
                ),
            ])));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (answers, calls, stopping) = (responses.clone(), requests.clone(), stop.clone());
            let worker = thread::spawn(move || {
                while !stopping.load(Ordering::Relaxed) {
                    let Some(request) = server.recv_timeout(Duration::from_millis(20)).unwrap()
                    else {
                        continue;
                    };
                    assert!(
                        !request
                            .headers()
                            .iter()
                            .any(|header| header.field.equiv("Authorization"))
                    );
                    let url = request.url().to_string();
                    calls.lock().unwrap().push(url.clone());
                    let (status, body) = answers
                        .lock()
                        .unwrap()
                        .get(&url)
                        .cloned()
                        .unwrap_or((404, Vec::new()));
                    let _ = request
                        .respond(tiny_http::Response::from_data(body).with_status_code(status));
                }
            });
            Self {
                root,
                path,
                releases: Releases {
                    client: Client::builder()
                        .no_proxy()
                        .timeout(Duration::from_secs(2))
                        .build()
                        .unwrap(),
                    root: origin,
                },
                responses,
                requests,
                stop,
                worker: Some(worker),
                original,
            }
        }

        fn response(&self, suffix: &str, status: u16, body: Vec<u8>) {
            self.responses
                .lock()
                .unwrap()
                .insert(suffix.into(), (status, body));
        }

        fn install(&self) -> Result<Value> {
            self.releases.install(&self.path, PLATFORM)
        }

        fn assert_private_state_unchanged(&self) {
            assert_eq!(
                fs::read_to_string(self.root.path().join("credentials.json")).unwrap(),
                "synthetic-private-credentials"
            );
            assert_eq!(
                fs::read_to_string(self.root.path().join(".cortex/config.toml")).unwrap(),
                "brain_id = 'org_1111111111111111'\n"
            );
            assert!(!fs::read_dir(self.root.path()).unwrap().any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            }));
        }

        fn assert_preserved(&self) {
            assert_eq!(fs::read(&self.path).unwrap(), self.original);
            self.assert_private_state_unchanged();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            self.worker.take().unwrap().join().unwrap();
        }
    }

    #[test]
    fn update_verifies_and_replaces_actual_executable_without_changing_private_state() {
        let fixture = Fixture::new("0.5.0", "0.6.0");
        let result = fixture.install().unwrap();
        assert_eq!(
            result,
            json!({"status":"updated", "previousVersion":"0.5.0", "version":"0.6.0", "path":fixture.path})
        );
        assert_eq!(
            executable_version(&fixture.path).unwrap().to_string(),
            "0.6.0"
        );
        assert_eq!(
            fs::metadata(&fixture.path).unwrap().permissions().mode() & 0o777,
            0o755
        );
        fixture.assert_private_state_unchanged();
        let result = fixture.install().unwrap();
        assert_eq!(result["status"], "current");
        assert_eq!(result["previousVersion"], "0.6.0");
        assert_eq!(fixture.requests.lock().unwrap().len(), 4);
    }

    #[test]
    fn update_rereads_installed_version_and_never_downgrades_or_reinstalls() {
        for current in ["0.6.0", "1.0.0", "0.6.0+local.2"] {
            let fixture = Fixture::new(current, "0.6.0");
            let result = fixture.install().unwrap();
            assert_eq!(result["status"], "current");
            assert_eq!(result["version"], current);
            assert_eq!(
                *fixture.requests.lock().unwrap(),
                ["/latest/download/latest.txt"]
            );
            fixture.assert_preserved();
        }
    }

    #[test]
    fn update_promotes_a_prerelease_to_the_same_stable_version() {
        let fixture = Fixture::new("0.6.0-beta.1", "0.6.0");
        assert_eq!(fixture.install().unwrap()["status"], "updated");
        assert_eq!(
            executable_version(&fixture.path).unwrap().to_string(),
            "0.6.0"
        );
    }

    #[test]
    fn malformed_or_unavailable_release_metadata_preserves_the_installation() {
        for (status, body) in [
            (200, b"not-a-version".to_vec()),
            (200, b"0.7.0-beta.1".to_vec()),
            (200, vec![b'1'; 1025]),
            (200, vec![255]),
            (503, b"unavailable".to_vec()),
        ] {
            let fixture = Fixture::new("0.5.0", "0.6.0");
            fixture.response("/latest/download/latest.txt", status, body);
            assert_eq!(fixture.install().unwrap_err().code, "update");
            fixture.assert_preserved();
        }
    }

    #[test]
    fn corrupt_misidentified_or_unavailable_downloads_preserve_the_installation() {
        let binary_path = format!("/download/v0.6.0/cortex-{PLATFORM}");
        let checksum_path = format!("{binary_path}.sha256");
        for (path, status, body) in [
            (binary_path.clone(), 200, b"corrupted".to_vec()),
            (binary_path.clone(), 404, Vec::new()),
            (
                checksum_path.clone(),
                200,
                format!("{}  cortex-other\n", "a".repeat(64)).into_bytes(),
            ),
            (checksum_path, 200, b"invalid-checksum".to_vec()),
        ] {
            let fixture = Fixture::new("0.5.0", "0.6.0");
            fixture.response(&path, status, body);
            assert_eq!(fixture.install().unwrap_err().code, "update");
            fixture.assert_preserved();
        }
    }

    #[test]
    fn checksum_is_verified_before_running_the_downloaded_executable() {
        let fixture = Fixture::new("0.5.0", "0.6.0");
        let marker = fixture.root.path().join("executed");
        let binary = format!(
            "#!/bin/sh\ntouch '{}'\nprintf 'cortex 0.6.0\\n'\n",
            marker.display()
        )
        .into_bytes();
        fixture.response(&format!("/download/v0.6.0/cortex-{PLATFORM}"), 200, binary);
        assert!(fixture.install().is_err());
        assert!(!marker.exists());
        fixture.assert_preserved();
    }

    #[test]
    fn downloaded_executable_must_identify_the_exact_release_version() {
        let fixture = Fixture::new("0.5.0", "0.6.0");
        let binary = executable("0.7.0");
        fixture.response(
            &format!("/download/v0.6.0/cortex-{PLATFORM}"),
            200,
            binary.clone(),
        );
        fixture.response(
            &format!("/download/v0.6.0/cortex-{PLATFORM}.sha256"),
            200,
            format!("{:x}  cortex-{PLATFORM}\n", Sha256::digest(binary)).into_bytes(),
        );
        assert!(
            fixture
                .install()
                .unwrap_err()
                .message
                .contains("does not match")
        );
        fixture.assert_preserved();
    }

    #[test]
    fn exclusive_update_lock_prevents_overlapping_replacements() {
        let fixture = Fixture::new("0.5.0", "0.6.0");
        let _guard = config::lock(&fixture.root.path().join(".cortex.update.lock")).unwrap();
        assert_eq!(fixture.install().unwrap_err().code, "busy");
        assert!(fixture.requests.lock().unwrap().is_empty());
        fixture.assert_preserved();
    }

    #[test]
    fn official_downloader_refuses_http_even_at_a_test_boundary() {
        let fixture = Fixture::new("0.5.0", "0.6.0");
        let mut releases = Releases::official().unwrap();
        releases.root = fixture.releases.root.clone();
        assert_eq!(
            releases
                .text("/latest/download/latest.txt")
                .unwrap_err()
                .code,
            "network"
        );
        assert!(fixture.requests.lock().unwrap().is_empty());
        fixture.assert_preserved();
    }
}
