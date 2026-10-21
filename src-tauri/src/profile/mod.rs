pub mod link_parser;
pub mod subscription;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Vless,
    Vmess,
    Trojan,
    Shadowsocks,
}

/// Flat description of one server, independent of the share-link format it came from.
/// Optional fields stay `None` when the link did not set them, so the config builder
/// can leave Xray's own defaults in place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Server {
    pub protocol: Option<Protocol>,
    pub address: String,
    pub port: u16,
    /// UUID for VLESS/VMess, password for Trojan/Shadowsocks.
    pub credential: String,
    /// VLESS: flow (e.g. xtls-rprx-vision).
    pub flow: Option<String>,
    /// VLESS: encryption ("none" or a post-quantum key string).
    pub encryption: Option<String>,
    /// VMess: alterId.
    pub alter_id: Option<u32>,
    /// VMess security or Shadowsocks method.
    pub method: Option<String>,

    /// raw | xhttp | ws | grpc | httpupgrade
    pub network: String,
    pub header_type: Option<String>,
    pub host: Option<String>,
    pub path: Option<String>,
    pub service_name: Option<String>,
    pub authority: Option<String>,
    /// xhttp mode or grpc "multi"/"gun".
    pub mode: Option<String>,
    /// xhttp "extra" object, kept verbatim.
    pub extra: Option<serde_json::Value>,

    /// none | tls | reality
    pub security: String,
    pub sni: Option<String>,
    pub fingerprint: Option<String>,
    pub alpn: Vec<String>,
    pub allow_insecure: bool,
    pub public_key: Option<String>,
    pub short_id: Option<String>,
    pub spider_x: Option<String>,
    pub mldsa65_verify: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub subscription_id: Option<String>,
    /// The share link the profile was imported from, for copying back out.
    pub link: String,
    pub server: Server,
}

impl Profile {
    /// Identity used to keep the selection when a subscription refresh replaces its profiles.
    pub fn identity(&self) -> (String, u16, String) {
        (self.server.address.to_lowercase(), self.server.port, self.server.credential.clone())
    }
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
