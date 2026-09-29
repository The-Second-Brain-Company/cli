use serde::Serialize;
use serde_json::Value;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Serialize)]
pub struct Error {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            status: None,
            details: None,
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_input", message)
    }
    pub fn exit_code(&self) -> i32 {
        match self.code.as_str() {
            "invalid_input" | "config" => 2,
            "authentication" => 3,
            "permission_denied" => 4,
            "conflict" => 5,
            _ => 1,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::new(
            "io",
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                "Local file or socket access was denied. Use the host's permission flow to allow access to the required files and service connection."
            } else {
                "Could not access a required local file or socket"
            },
        )
    }
}
impl From<serde_json::Error> for Error {
    fn from(_: serde_json::Error) -> Self {
        Self::invalid("Expected valid JSON")
    }
}
impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Self::new(
            "network",
            if error.is_timeout() {
                "Request timed out. Preserve the original Brain, retry ID, and input after an uncertain write."
            } else {
                "Request failed. Check the service origin and connection. Preserve retry identity after an uncertain write."
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn local_permission_errors_offer_recovery_without_exposing_internal_details() {
        let error = Error::from(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "synthetic-private-detail",
        ));
        assert_eq!(error.code, "io");
        assert_ne!(error.exit_code(), 0);
        assert!(error.message.contains("permission"));
        assert!(
            !serde_json::to_string(&error)
                .unwrap()
                .contains("synthetic-private-detail")
        );
    }
}
