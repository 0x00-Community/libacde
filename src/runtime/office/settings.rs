use crate::runtime::failure::{Failure, Outcome};
use std::os::unix::fs::MetadataExt;

const DEFAULT_DOCUMENT_SERVER: &str = "http://127.0.0.1:8085";
const DEFAULT_BRIDGE_URL: &str = "http://127.0.0.1:7979";

pub struct OfficeSettings {
    pub document_server: String,
    pub socket: String,
    pub bridge_url: String,
    pub env_file: String,
    secret_override: Option<String>,
}

impl OfficeSettings {
    fn origin(value: &str, name: &str) -> Outcome<String> {
        let url = url::Url::parse(value).map_err(|_| Failure::office_config(format!("{name} is not a URL")))?;
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err(Failure::office_config(format!("{name} must be http(s)")));
        }
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() || !url.username().is_empty() || url.password().is_some() {
            return Err(Failure::office_config(format!("{name} must be an origin like http://127.0.0.1:8085")));
        }
        Ok(url.as_str().trim_end_matches('/').to_owned())
    }

    pub fn from_env() -> Outcome<Self> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let home = std::env::home_dir().map(|h| h.to_string_lossy().into_owned()).unwrap_or_default();
        let document_server = Self::origin(&var("AGENTRT_OFFICE_DS_URL").unwrap_or_else(|| DEFAULT_DOCUMENT_SERVER.into()), "AGENTRT_OFFICE_DS_URL")?;
        let bridge_url = Self::origin(&var("AGENTRT_OFFICE_BRIDGE_URL").unwrap_or_else(|| DEFAULT_BRIDGE_URL.into()), "AGENTRT_OFFICE_BRIDGE_URL")?;
        let runtime = var("XDG_RUNTIME_DIR").unwrap_or_else(|| format!("/run/user/{}", rustix::process::getuid().as_raw()));
        let config = var("XDG_CONFIG_HOME").unwrap_or_else(|| format!("{home}/.config"));
        Ok(Self {
            document_server,
            socket: var("AGENTRT_OFFICE_SOCKET").unwrap_or_else(|| format!("{runtime}/acde/office/office.sock")),
            bridge_url,
            env_file: var("AGENTRT_OFFICE_ENV_FILE").unwrap_or_else(|| format!("{config}/agentos/office/ds.env")),
            secret_override: var("AGENTRT_OFFICE_JWT_SECRET"),
        })
    }

    pub fn secret_source(&self) -> &str {
        if self.secret_override.is_some() { "AGENTRT_OFFICE_JWT_SECRET" } else { &self.env_file }
    }

    pub fn jwt_secret(&self) -> Outcome<String> {
        let secret = match &self.secret_override {
            Some(secret) => secret.clone(),
            None => self.secret_from_file()?,
        };
        if secret.encode_utf16().count() < 32 {
            return Err(Failure::office_config(format!("JWT secret from {} is shorter than 32 characters", self.secret_source())));
        }
        Ok(secret)
    }

    fn secret_from_file(&self) -> Outcome<String> {
        let missing = || Failure::office_config(format!("no JWT secret: {} is missing; run /usr/lib/acdes/office-ds.sh up first", self.env_file));
        let info = std::fs::metadata(&self.env_file).map_err(|_| missing())?;
        if info.mode() & 0o077 != 0 {
            return Err(Failure::office_config(format!("{} must not be readable by group or others (chmod 600)", self.env_file)));
        }
        if info.uid() != rustix::process::getuid().as_raw() {
            return Err(Failure::office_config(format!("{} must belong to the user running agentrt", self.env_file)));
        }
        let text = std::fs::read_to_string(&self.env_file).map_err(|_| missing())?;
        for line in text.split('\n') {
            let Some(rest) = line.trim_start().strip_prefix("JWT_SECRET") else { continue };
            let Some(value) = rest.trim_start().strip_prefix('=') else { continue };
            let value = value.trim();
            for quote in ['\'', '"'] {
                if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
                    return Ok(value[1..value.len() - 1].to_owned());
                }
            }
            return Ok(value.to_owned());
        }
        Err(Failure::office_config(format!("{} has no JWT_SECRET line", self.env_file)))
    }
}
