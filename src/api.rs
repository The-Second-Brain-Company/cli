use crate::{
    auth, config,
    error::{Error, Result},
};
use reqwest::{Method, blocking::Client};
use serde_json::Value;

pub struct Api {
    pub origin: String,
    client: Client,
}

impl Api {
    pub fn new(origin: String) -> Result<Self> {
        let client = auth::client(&origin)?;
        Ok(Self { origin, client })
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
