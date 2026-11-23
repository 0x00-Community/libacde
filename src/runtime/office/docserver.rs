use super::Credentials;
use crate::runtime::failure::{Failure, Outcome};
use futures_util::StreamExt;
use ntex::client::ClientResponse;
use ntex::client::error::ClientError;
use ntex::time::Millis;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

enum Unanswered {
    Client(ClientError),
    TimedOut,
}

pub struct DocumentServer {
    pub url: String,
    credentials: Option<Arc<Credentials>>,
    problem: Option<String>,
}

impl DocumentServer {
    fn command_error(code: i64) -> String {
        match code {
            1 => "document key is missing or no document with such key could be found".into(),
            2 => "callback url not correct".into(),
            3 => "internal server error".into(),
            4 => "no changes were applied to the document before the forcesave command was received".into(),
            5 => "command not correct".into(),
            6 => "invalid token".into(),
            other => format!("error {other}"),
        }
    }

    fn reason(error: &Unanswered) -> String {
        match error {
            Unanswered::TimedOut | Unanswered::Client(ClientError::Timeout) => "timed out".into(),
            Unanswered::Client(ClientError::Connect(_)) => "ECONNREFUSED".into(),
            Unanswered::Client(error) => error.to_string(),
        }
    }

    pub fn new(url: &str, credentials: Option<Arc<Credentials>>, problem: Option<String>) -> Self {
        Self { url: url.to_owned(), credentials, problem }
    }

    async fn send(request: ntex::client::ClientRequest, body: Option<String>, timeout: Duration) -> Result<ClientResponse, Unanswered> {
        let request = request.timeout(Millis::ZERO);
        let sent = async {
            match body {
                Some(body) => request.send_body(body).await,
                None => request.send().await,
            }
        };
        match ntex::time::timeout(timeout, sent).await {
            Ok(response) => response.map_err(|error| Unanswered::Client(error.into_error())),
            Err(()) => Err(Unanswered::TimedOut),
        }
    }

    async fn body(response: &ClientResponse, limit: usize) -> Option<Vec<u8>> {
        response.body().limit(limit).timeout(Millis::ZERO).await.ok().map(|body| body.to_vec())
    }

    fn local(&self) -> bool {
        let host = self.url.split("://").nth(1).unwrap_or(&self.url).split(['/', ':']).next().unwrap_or_default();
        matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "")
    }

    pub async fn status(&self) -> Value {
        let failed = |problem: &str, message: String| json!({"ok": false, "url": self.url, "version": null, "problem": problem, "message": message});
        let timeout = Duration::from_millis(2500);
        let health = async {
            let response = Self::send(crate::runtime::http::client().get(format!("{}/healthcheck", self.url)), None, timeout).await?;
            let text = Self::body(&response, 64 * 1024).await.unwrap_or_default();
            Ok((response.status(), String::from_utf8_lossy(&text).trim().to_owned()))
        };
        let health = ntex::time::timeout(timeout, health).await.unwrap_or(Err(Unanswered::TimedOut));
        match health {
            Err(_) if self.local() && !containers() => {
                return failed("missing", format!("Document Server at {} is not reachable and this machine has neither docker nor podman to run it", self.url));
            }
            Err(error) => return failed("unreachable", format!("Document Server at {} is not reachable ({})", self.url, Self::reason(&error))),
            Ok((status, text)) => {
                if !status.is_success() || text != "true" {
                    let head: String = text.chars().take(80).collect();
                    return failed("unhealthy", format!("Document Server at {} is not healthy yet (HTTP {}: {head})", self.url, status.as_u16()));
                }
            }
        }
        if self.credentials.is_none() {
            return failed("secret", self.problem.clone().unwrap_or_else(|| "no JWT secret configured".into()));
        }
        match self.command(json!({"c": "version"}), timeout).await {
            Ok(reply) => json!({"ok": true, "url": self.url, "version": reply["version"].as_str(), "problem": null, "message": null}),
            Err(failure) => failed("rejected", format!("Document Server at {} refused the version command: {}", self.url, failure.message)),
        }
    }

    pub async fn forcesave(&self, key: &str) -> Outcome<String> {
        match self.command(json!({"c": "forcesave", "key": key, "userdata": "agentrt-office"}), Duration::from_secs(10)).await {
            Ok(_) => Ok("requested".into()),
            Err(failure) if failure.message == format!("Document Server: {}", Self::command_error(4)) => Ok("no_changes".into()),
            Err(failure) => Err(failure),
        }
    }

    async fn command(&self, body: Value, timeout: Duration) -> Outcome<Value> {
        let credentials = self.credentials.as_ref();
        let jwt = &credentials.ok_or_else(|| Failure::http(503, self.problem.clone().unwrap_or_else(|| "no JWT secret configured".into())))?.jwt;
        let mut signed = body.clone();
        signed["token"] = jwt.sign(&body, Some(300)).into();
        let request = crate::runtime::http::client()
            .post(format!("{}/command", self.url))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", jwt.sign(&json!({"payload": body}), Some(300))));
        let exchange = async {
            let response = Self::send(request, Some(signed.to_string()), timeout).await?;
            Ok((response.status(), Self::body(&response, 1024 * 1024).await))
        };
        let (status, reply) = ntex::time::timeout(timeout, exchange)
            .await
            .unwrap_or(Err(Unanswered::TimedOut))
            .map_err(|e| Failure::http(503, format!("Document Server is not reachable ({})", Self::reason(&e))))?;
        let reply: Option<Value> = reply.and_then(|b| serde_json::from_slice(&b).ok()).filter(Value::is_object);
        let Some(reply) = reply.filter(|_| status.is_success()) else {
            return Err(Failure::http(502, format!("Document Server command failed with HTTP {}", status.as_u16())));
        };
        let code = match &reply["error"] {
            Value::Number(n) => n.as_i64().unwrap_or(0),
            Value::String(s) => s.parse().unwrap_or(0),
            _ => 0,
        };
        if code != 0 {
            return Err(Failure::http(502, format!("Document Server: {}", Self::command_error(code))));
        }
        Ok(reply)
    }

    pub async fn download(&self, url: &str, max: usize) -> Outcome<Vec<u8>> {
        let source = url::Url::parse(url).map_err(|_| Failure::http(502, "Document Server sent an invalid file URL"))?;
        let path = source.path();
        let at = path.find("/cache/files/").filter(|_| !path.contains("/../"));
        let at = at.ok_or_else(|| Failure::http(502, "Document Server sent a file URL outside its cache"))?;
        let query = source.query().map_or_else(String::new, |q| format!("?{q}"));
        let target = format!("{}{}{query}", self.url, &path[at..]);
        let failed = |e: &Unanswered| Failure::http(502, format!("downloading the edited file failed ({})", Self::reason(e)));
        let fetch = async {
            let mut response = Self::send(crate::runtime::http::client().get(target), None, Duration::from_secs(60)).await.map_err(|e| failed(&e))?;
            if !response.status().is_success() {
                return Err(Failure::http(502, format!("downloading the edited file failed with HTTP {}", response.status().as_u16())));
            }
            let mut data = Vec::new();
            while let Some(chunk) = response.next().await {
                let chunk = chunk.map_err(|e| Failure::http(502, format!("downloading the edited file failed ({e})")))?;
                data.extend_from_slice(&chunk);
                if data.len() > max {
                    return Err(Failure::http(413, "the edited file exceeds 64 MiB"));
                }
            }
            Ok(data)
        };
        ntex::time::timeout(Duration::from_secs(60), fetch).await.unwrap_or_else(|()| Err(failed(&Unanswered::TimedOut)))
    }
}

fn containers() -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| ["docker", "podman"].iter().any(|tool| dir.join(tool).is_file())))
}
