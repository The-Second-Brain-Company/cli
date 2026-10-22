use crate::{
    auth, config,
    error::{Error, Result},
};
use reqwest::{Method, blocking::Client};
use semver::Version;
use serde::Serialize;
use serde_json::Value;
use std::cell::RefCell;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateHint {
    current_version: String,
    latest_version: String,
    minimum_version: String,
    command: &'static str,
}

impl UpdateHint {
    fn from_headers(headers: &reqwest::header::HeaderMap, current: &str) -> Option<Self> {
        let latest =
            Version::parse(headers.get("Cortex-CLI-Latest-Version")?.to_str().ok()?).ok()?;
        let minimum =
            Version::parse(headers.get("Cortex-CLI-Minimum-Version")?.to_str().ok()?).ok()?;
        let current = Version::parse(current).ok()?;
        if !latest.pre.is_empty()
            || !minimum.pre.is_empty()
            || latest.cmp_precedence(&minimum).is_lt()
            || (!latest.cmp_precedence(&current).is_gt()
                && !minimum.cmp_precedence(&current).is_gt())
        {
            return None;
        }
        Some(Self {
            current_version: current.to_string(),
            latest_version: latest.to_string(),
            minimum_version: minimum.to_string(),
            command: "cortex update",
        })
    }
}

pub struct Api {
    pub origin: String,
    client: Client,
    update: RefCell<Option<UpdateHint>>,
}

impl Api {
    pub fn new(origin: String) -> Result<Self> {
        let client = auth::client(&origin)?;
        Ok(Self {
            origin,
            client,
            update: RefCell::new(None),
        })
    }
    pub fn update_hint(&self) -> Option<UpdateHint> {
        self.update.borrow().clone()
    }
    pub fn request(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        input: Option<&Value>,
    ) -> Result<Value> {
        let token = auth::access_token(&self.origin, &self.client)?;
        let mut request = self
            .client
            .request(method, format!("{}/app/api/cli{path}", self.origin))
            .bearer_auth(token)
            .query(query);
        if let Some(input) = input {
            request = request.json(input);
        }
        let response = request.send()?;
        if let Some(hint) = UpdateHint::from_headers(response.headers(), env!("CARGO_PKG_VERSION"))
        {
            self.update.replace(Some(hint));
        }
        if response.status().as_u16() == 204 {
            return Ok(serde_json::json!({"completed": true}));
        }
        auth::decode(response)
    }
    pub fn get(&self, path: &str) -> Result<Value> {
        self.request(Method::GET, path, &[], None)
    }
    pub fn org(
        &self,
        brain: &str,
        method: Method,
        suffix: &str,
        query: &[(&str, String)],
        input: Option<&Value>,
    ) -> Result<Value> {
        config::identifier(brain, "org")?;
        self.request(method, &format!("/orgs/{brain}{suffix}"), query, input)
    }
    pub fn whoami(&self, brain: &str) -> Result<Value> {
        let identity = self.org(brain, Method::GET, "/membership", &[], None)?;
        if identity.pointer("/organization/id").and_then(Value::as_str) != Some(brain) {
            return Err(Error::new(
                "conflict",
                "Server returned a different Brain; no local selection changed",
            ));
        }
        Ok(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    fn headers(latest: &str, minimum: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Cortex-CLI-Latest-Version",
            HeaderValue::from_str(latest).unwrap(),
        );
        headers.insert(
            "Cortex-CLI-Minimum-Version",
            HeaderValue::from_str(minimum).unwrap(),
        );
        headers
    }

    #[test]
    fn update_hints_compare_semantic_precedence_and_require_consistent_stable_versions() {
        let hint = UpdateHint::from_headers(&headers("1.0.0", "0.6.0"), "0.6.0-alpha.1").unwrap();
        let value = serde_json::to_value(hint).unwrap();
        assert_eq!(value["currentVersion"], "0.6.0-alpha.1");
        assert_eq!(value["minimumVersion"], "0.6.0");
        assert_eq!(value["latestVersion"], "1.0.0");
        assert_eq!(value["command"], "cortex update");
        for (latest, minimum, current) in [
            ("0.6.0+build.2", "0.5.0", "0.6.0+build.1"),
            ("0.6.0", "0.5.0", "0.7.0"),
            ("0.6.0", "1.0.0", "0.5.0"),
            ("0.7.0-beta.1", "0.5.0", "0.6.0"),
            ("1.0.0", "invalid", "0.6.0"),
            ("invalid", "0.5.0", "0.6.0"),
        ] {
            assert!(UpdateHint::from_headers(&headers(latest, minimum), current).is_none());
        }
        assert!(UpdateHint::from_headers(&HeaderMap::new(), "0.6.0").is_none());
    }
}
