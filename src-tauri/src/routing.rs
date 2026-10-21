//! Split-tunneling settings and normalization of user input into Xray rule values.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ListMode {
    /// Everything goes through the VPN except the listed apps/domains/IPs.
    #[default]
    Exclude,
    /// Only the listed apps/domains/IPs go through the VPN.
    Include,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RoutingSettings {
    pub list_mode: ListMode,
    /// Executable paths (`C:/x/app.exe`), folders (`C:/Games/`) or bare process names.
    pub apps: Vec<String>,
    pub domains: Vec<String>,
    pub ips: Vec<String>,
    pub ru_direct: bool,
    pub lan_direct: bool,
    pub block_ads: bool,
}

impl Default for RoutingSettings {
    fn default() -> Self {
        Self {
            list_mode: ListMode::Exclude,
            apps: Vec::new(),
            domains: Vec::new(),
            ips: Vec::new(),
            ru_direct: true,
            lan_direct: true,
            block_ads: false,
        }
    }
}

pub const RU_DOMAINS: &[&str] = &["geosite:category-ru", "domain:ru", "domain:su", "domain:xn--p1ai"];
pub const RU_IPS: &[&str] = &["geoip:ru"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleKind {
    App,
    Domain,
    Ip,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RuleError {
    // Displayed as codes; the UI translates them.
    #[error("EMPTY")]
    Empty,
    #[error("INVALID_DOMAIN")]
    InvalidDomain,
    #[error("INVALID_IP")]
    InvalidIp,
}

pub fn normalize(kind: RuleKind, input: &str) -> Result<String, RuleError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(RuleError::Empty);
    }
    match kind {
        RuleKind::App => Ok(normalize_app(input)),
        RuleKind::Domain => normalize_domain(input),
        RuleKind::Ip => normalize_ip(input),
    }
}

/// Paths use forward slashes and an upper-case drive letter, as Xray compares them
/// case-sensitively against what Windows reports. Bare names lose `.exe`, which Xray
/// strips from process names before matching.
fn normalize_app(input: &str) -> String {
    let s = input.trim_matches('"').replace('\\', "/");
    if s.contains('/') {
        let mut chars: Vec<char> = s.chars().collect();
        if chars.len() > 1 && chars[1] == ':' {
            chars[0] = chars[0].to_ascii_uppercase();
        }
        chars.into_iter().collect()
    } else {
        match s.len().checked_sub(4) {
            Some(cut) if s[cut..].eq_ignore_ascii_case(".exe") => s[..cut].to_string(),
            _ => s,
        }
    }
}

const DOMAIN_PREFIXES: &[&str] = &["domain:", "full:", "regexp:", "keyword:", "geosite:", "ext:"];

fn normalize_domain(input: &str) -> Result<String, RuleError> {
    if let Some(prefix) = DOMAIN_PREFIXES.iter().find(|p| input.starts_with(**p)) {
        let value = &input[prefix.len()..];
        if value.is_empty() {
            return Err(RuleError::InvalidDomain);
        }
        return match *prefix {
            "domain:" | "full:" => Ok(format!("{prefix}{}", to_ascii_domain(value)?)),
            _ => Ok(input.to_string()),
        };
    }
    // Accept pasted URLs: strip scheme, path, port and a leading "*." wildcard.
    let mut host = input;
    if let Some((_, rest)) = host.split_once("://") {
        host = rest;
    }
    host = host.split(['/', '?', '#']).next().unwrap_or(host);
    host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    host = host.split(':').next().unwrap_or(host);
    host = host.trim_start_matches("*.").trim_start_matches('.');
    Ok(format!("domain:{}", to_ascii_domain(host)?))
}

/// Lower-cases and converts internationalized names (e.g. `.рф`) to punycode.
fn to_ascii_domain(value: &str) -> Result<String, RuleError> {
    let ascii = idna::domain_to_ascii(value).map_err(|_| RuleError::InvalidDomain)?;
    let valid = !ascii.is_empty()
        && ascii.split('.').all(|label| {
            !label.is_empty() && label.len() <= 63 && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        });
    if valid { Ok(ascii) } else { Err(RuleError::InvalidDomain) }
}

fn normalize_ip(input: &str) -> Result<String, RuleError> {
    if let Some(code) = input.strip_prefix("geoip:") {
        let code = code.trim().to_ascii_lowercase();
        return if !code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '!') {
            Ok(format!("geoip:{code}"))
        } else {
            Err(RuleError::InvalidIp)
        };
    }
    if input.starts_with("ext:") {
        return Ok(input.to_string());
    }
    let (addr, prefix) = match input.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (input, None),
    };
    let ip: IpAddr = addr.trim().parse().map_err(|_| RuleError::InvalidIp)?;
    match prefix {
        None => Ok(ip.to_string()),
        Some(p) => {
            let len: u8 = p.trim().parse().map_err(|_| RuleError::InvalidIp)?;
            let max = if ip.is_ipv4() { 32 } else { 128 };
            if len > max {
                return Err(RuleError::InvalidIp);
            }
            Ok(format!("{ip}/{len}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps() {
        assert_eq!(normalize(RuleKind::App, r"c:\Program Files\Steam\steam.exe").unwrap(), "C:/Program Files/Steam/steam.exe");
        assert_eq!(normalize(RuleKind::App, r"D:\Games\").unwrap(), "D:/Games/");
        assert_eq!(normalize(RuleKind::App, "Telegram.exe").unwrap(), "Telegram");
        assert_eq!(normalize(RuleKind::App, "curl").unwrap(), "curl");
    }

    #[test]
    fn domains() {
        assert_eq!(normalize(RuleKind::Domain, "Example.COM").unwrap(), "domain:example.com");
        assert_eq!(normalize(RuleKind::Domain, "https://www.youtube.com/watch?v=1").unwrap(), "domain:www.youtube.com");
        assert_eq!(normalize(RuleKind::Domain, "*.google.com").unwrap(), "domain:google.com");
        assert_eq!(normalize(RuleKind::Domain, "яндекс.рф").unwrap(), "domain:xn--d1acpjx3f.xn--p1ai");
        assert_eq!(normalize(RuleKind::Domain, "full:api.example.com").unwrap(), "full:api.example.com");
        assert_eq!(normalize(RuleKind::Domain, "geosite:youtube").unwrap(), "geosite:youtube");
        assert_eq!(normalize(RuleKind::Domain, "regexp:^.*\\.ru$").unwrap(), "regexp:^.*\\.ru$");
        assert_eq!(normalize(RuleKind::Domain, "bad domain"), Err(RuleError::InvalidDomain));
        assert_eq!(normalize(RuleKind::Domain, "  "), Err(RuleError::Empty));
    }

    #[test]
    fn ips() {
        assert_eq!(normalize(RuleKind::Ip, "10.0.0.0/8").unwrap(), "10.0.0.0/8");
        assert_eq!(normalize(RuleKind::Ip, "1.1.1.1").unwrap(), "1.1.1.1");
        assert_eq!(normalize(RuleKind::Ip, "2001:db8::/32").unwrap(), "2001:db8::/32");
        assert_eq!(normalize(RuleKind::Ip, "geoip:RU").unwrap(), "geoip:ru");
        assert_eq!(normalize(RuleKind::Ip, "1.2.3.4/33"), Err(RuleError::InvalidIp));
        assert_eq!(normalize(RuleKind::Ip, "300.1.1.1"), Err(RuleError::InvalidIp));
    }
}
