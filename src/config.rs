use crate::error::{Error, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const DEFAULT_ORIGIN: &str = "http://second-brain.localhost:1355";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[serde(alias = "brain_id")]
    pub cortex_id: String,
}

pub fn identifier(value: &str, prefix: &str) -> Result<String> {
    let suffix = value.strip_prefix(&format!("{prefix}_")).unwrap_or("");
    if suffix.len() != 16 || !suffix.bytes().all(|c| c.is_ascii_alphanumeric()) {
        return Err(Error::invalid(format!(
            "Expected {prefix}_ followed by 16 letters or digits"
        )));
    }
    Ok(value.into())
}

pub fn origin(value: &str) -> Result<String> {
    let url = reqwest::Url::parse(value).map_err(|_| Error::invalid("Invalid service origin"))?;
    let host = url.host_str().unwrap_or("");
    let local = host == "localhost"
        || host.ends_with(".localhost")
        || host == "127.0.0.1"
        || host == "[::1]";
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || !(url.scheme() == "https" || (url.scheme() == "http" && local))
    {
        return Err(Error::invalid(
            "Use an HTTPS origin or HTTP localhost origin, without a path, credentials, query, or fragment",
        ));
    }
    Ok(url.origin().ascii_serialization())
}

pub fn project(value: Option<&str>) -> Result<PathBuf> {
    let path = value.map(PathBuf::from).unwrap_or(std::env::current_dir()?);
    if !path.is_dir() {
        return Err(Error::invalid("Project directory does not exist"));
    }
    Ok(path.canonicalize()?)
}

pub fn selected(project: &Path) -> Result<Option<String>> {
    let directory = if project.join(".cortex/config.toml").exists()
        || project.join(".cortex/config.json").exists()
    {
        project.join(".cortex")
    } else {
        project.join(".brain")
    };
    let path = directory.join("config.toml");
    if !path.exists() {
        if directory.join("config.json").exists() {
            return Err(Error::new(
                "config",
                "This project has only plugin JSON selection. Select its intended Cortex explicitly with cortex use to save CLI configuration.",
            ));
        }
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    let selection: Selection = toml::from_str(&raw).map_err(|_| {
        Error::new(
            "config",
            "Invalid .cortex/config.toml; expected only cortex_id = \"org_...\"",
        )
    })?;
    Ok(Some(identifier(&selection.cortex_id, "org")?))
}

pub fn auth_dir() -> Result<PathBuf> {
    let base = if let Some(path) =
        std::env::var_os("CORTEX_HOME").or_else(|| std::env::var_os("BRAIN_HOME"))
    {
        PathBuf::from(path)
    } else {
        let root = if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
            PathBuf::from(path)
        } else {
            PathBuf::from(std::env::var_os("HOME").ok_or_else(|| {
                Error::new(
                    "config",
                    "Set CORTEX_HOME to a private credential directory",
                )
            })?)
            .join(".config")
        };
        let current = root.join("cortex");
        let legacy = root.join("second-brain");
        if !current.exists() && legacy.exists() {
            legacy
        } else {
            current
        }
    };
    private_dir(&base)?;
    Ok(base)
}

pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    reject_symlink(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn reject_symlink(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(Error::new(
            "config",
            "Refusing a symlink for local configuration or credentials",
        ));
    }
    Ok(())
}

pub fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    reject_symlink(path)?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = private_file(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn lock(path: &Path) -> Result<File> {
    reject_symlink(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    file.try_lock_exclusive().map_err(|_| {
        Error::new(
            "busy",
            "Another CLI operation is updating this local state; retry after it finishes",
        )
    })?;
    Ok(file)
}

pub fn read_input(path: &str, maximum: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    if path == "-" {
        std::io::stdin().take(maximum + 1).read_to_end(&mut bytes)?;
    } else {
        File::open(path)?
            .take(maximum + 1)
            .read_to_end(&mut bytes)?;
    }
    if bytes.len() as u64 > maximum {
        return Err(Error::invalid("Input exceeds the command's byte limit"));
    }
    Ok(bytes)
}
