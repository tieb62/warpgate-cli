use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::path::Path;
use ureq::typestate::WithoutBody;
use ureq::RequestBuilder;

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    warpgate_url: String,
    api_token: String,
    api_token_id: Option<String>,
    api_token_expires: Option<DateTime<Utc>>,
    refresh_frequency: u8,
}

impl Config {
    pub fn warpgate_url(&self) -> &str {
        &self.warpgate_url
    }

    fn refresh_token(&mut self, config_path: &Path) {
        let exp = Utc::now() + chrono::Duration::days(self.refresh_frequency as i64);
        let create_token_response = self.warpgate_create_api_token_req(
            "Warpgate CLI",
            exp,
        );

        if self.api_token_id.is_none() || self.api_token_expires.is_none() {
            println!(
                "A new API token was created and will be kept valid, you should now delete the original token (not in config file, only Warpgate UI)"
            );
        } else if let Some(api_token_id) = self.api_token_id.as_ref() {
            self.warpgate_delete_req(&format!(
                "/@warpgate/api/profile/api-tokens/{}",
                api_token_id
            ));
        }

        self.api_token = create_token_response.secret;
        self.api_token_id = Some(create_token_response.token.id);
        self.api_token_expires = Some(exp);
        save_config(self, config_path);
    }

    fn ensure_token(&mut self, force_refresh_token: bool, config_path: &Path) {
        if force_refresh_token
            || self.api_token_expires.is_none()
            || should_refresh(self.api_token_expires.unwrap(), self.refresh_frequency)
        {
            self.refresh_token(config_path);
        }
    }

    pub fn fetch_targets(
        &mut self,
        force_refresh_token: bool,
        config_path: &Path,
    ) -> HashMap<String, TargetKind> {
        self.ensure_token(force_refresh_token, config_path);

        let targets_response: Vec<ApiTarget> = self.warpgate_get_req("/@warpgate/api/targets");

        targets_response
            .into_iter()
            .map(|t| (t.name, t.kind))
            .collect()
    }

    pub fn fetch_info(&mut self, force_refresh_token: bool, config_path: &Path) -> WarpgateInfo {
        self.ensure_token(force_refresh_token, config_path);

        let info: ApiInfoResponse = self.warpgate_get_req("/@warpgate/api/info");

        let protocol_info: HashMap<TargetKind, ProtocolInfo> = info
            .external_hosts
            .into_iter()
            .filter(|(kind, _)| info.ports.get(kind).is_some_and(Option::is_some))
            .map(|(kind, host)| {
                let port = info.ports.get(&kind).unwrap().unwrap();
                (kind, ProtocolInfo { host, port })
            })
            .collect();

        WarpgateInfo {
            username: info.username,
            protocol_info,
        }
    }

    fn get_req_url(&self, path: &str) -> String {
        format!("{}{}", self.warpgate_url, path)
    }

    fn warpgate_get_req<T: DeserializeOwned>(&self, path: &str) -> T {
        self.warpgate_req(ureq::get(self.get_req_url(path)))
    }

    fn warpgate_delete_req(&self, path: &str) {
        ureq::delete(self.get_req_url(path))
            .header("X-Warpgate-Token", self.api_token.as_str())
            .call()
            .unwrap_or_else(|err| panic!("Failed to call Warpgate API, details : {:?}", err));
    }

    fn generate_create_api_token_body(label: &str, expiry: DateTime<Utc>) -> String {
        serde_json::to_string(&serde_json::json!({
            "label": label,
            "expiry": expiry.to_rfc3339(),
        }))
        .expect("Failed to serialize API token creation body")
    }

    fn warpgate_create_api_token_req(
        &self,
        label: &str,
        expiry: DateTime<Utc>,
    ) -> CreateApiTokenResponse {
        let full_url = self.get_req_url("/@warpgate/api/profile/api-tokens");
        ureq::post(&full_url)
            .header("X-Warpgate-Token", self.api_token.as_str())
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .send(Self::generate_create_api_token_body(label, expiry))
            .unwrap_or_else(|err| {
                panic!(
                    "Failed to POST Warpgate API at {}, details : {:?}",
                    full_url, err
                )
            })
            .body_mut()
            .read_json()
            .expect("Failed to read/parse Warpgate API response")
    }

    fn warpgate_req<T: DeserializeOwned>(&self, builder: RequestBuilder<WithoutBody>) -> T {
        builder
            .header("X-Warpgate-Token", self.api_token.as_str())
            .call()
            .unwrap_or_else(|err| panic!("Failed to call Warpgate API, details : {:?}", err))
            .body_mut()
            .read_json()
            .expect("Failed to read Warpgate API response")
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct CreateApiTokenResponse {
    token: ApiToken,
    secret: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ApiToken {
    id: String,
    label: String,
    created: DateTime<Utc>,
    expiry: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ApiTarget {
    name: String,
    description: String,
    kind: TargetKind,
    external_host: Option<String>,
    default_database_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ApiInfoResponse {
    external_hosts: HashMap<TargetKind, String>,
    ports: HashMap<TargetKind, Option<u16>>,
    username: String,
}

#[derive(Debug)]
pub struct WarpgateInfo {
    username: String,
    protocol_info: HashMap<TargetKind, ProtocolInfo>,
}

impl WarpgateInfo {
    pub fn get_protocol_info(&self, kind: &TargetKind) -> Option<&ProtocolInfo> {
        self.protocol_info.get(kind)
    }

    pub fn username(&self) -> &String {
        &self.username
    }
}

#[derive(Debug)]
pub struct ProtocolInfo {
    host: String,
    port: u16,
}

impl ProtocolInfo {
    pub fn host(&self) -> &String {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetKind {
    Http,
    Kubernetes,
    MySQL,
    Ssh,
    Postgres,
}

impl TargetKind {
    pub fn as_str(&self) -> &str {
        match self {
            TargetKind::Http => "HTTP",
            TargetKind::Kubernetes => "Kubernetes",
            TargetKind::MySQL => "MySQL",
            TargetKind::Ssh => "SSH",
            TargetKind::Postgres => "Postgres",
        }
    }
}

impl Serialize for TargetKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TargetKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.as_str().try_into().map_err(serde::de::Error::custom)
    }
}

impl TryFrom<&str> for TargetKind {
    type Error = String;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        //noinspection SpellCheckingInspection
        match s.to_lowercase().as_str() {
            "http" => Ok(Self::Http),
            "mysql" => Ok(Self::MySQL),
            "ssh" => Ok(Self::Ssh),
            "postgres" => Ok(Self::Postgres),
            "kubernetes" => Ok(Self::Kubernetes),
            _ => Err("Unknown target kind: {}".into()),
        }
    }
}

impl From<TargetKind> for String {
    fn from(kind: TargetKind) -> Self {
        kind.as_str().into()
    }
}

impl From<&TargetKind> for String {
    fn from(kind: &TargetKind) -> Self {
        kind.as_str().into()
    }
}

fn get_default_config() -> &'static str {
    include_str!("default-config.json")
}

pub fn get_config(path: &Path) -> Config {
    if !path.exists() {
        println!("Creating default config at {}", path.display());
        std::fs::write(path, get_default_config())
            .unwrap_or_else(|_| panic!("Failed to write config at {}", path.display()));
        std::process::exit(0);
    }
    let str_config = std::fs::read_to_string(path)
        .unwrap_or_else(|_| panic!("Failed to read config at {}", path.display()));
    serde_json::from_str(&str_config).expect("Failed to parse config")
}

pub fn save_config(config: &Config, path: &Path) {
    let json = serde_json::to_string_pretty(config).expect("Failed to serialize config");
    std::fs::write(path, json).expect("Failed to write config");
}

/// - `expiration_date` is when the token will expire
/// - `frequency` in days
fn should_refresh(expiration_date: DateTime<Utc>, frequency: u8) -> bool {
    let refresh_threshold = expiration_date - chrono::Duration::days((frequency / 2) as i64);
    Utc::now() >= refresh_threshold
}
