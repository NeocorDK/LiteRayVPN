//! Subscription download and parsing (3X-UI / v2rayN style: base64 or plain list of links).

use serde::{Deserialize, Serialize};

use super::link_parser::{decode_base64_lenient, parse_link};
use super::{new_id, Profile};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub upload: u64,
    pub download: u64,
    /// 0 means unlimited.
    pub total: u64,
    /// Unix seconds, `None` when the subscription never expires.
    pub expire: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub id: String,
    pub name: String,
    pub url: String,
    pub last_update: Option<i64>,
    /// Interval suggested by the server (profile-update-interval), hours.
    pub update_interval_hours: Option<u32>,
    pub userinfo: Option<UserInfo>,
    pub last_error: Option<String>,
    /// Links of the last refresh that this client cannot use (e.g. AmneziaWG).
    #[serde(default)]
    pub skipped: usize,
}

#[derive(Debug, Default)]
pub struct Fetched {
    pub profiles: Vec<Profile>,
    pub skipped: usize,
    pub userinfo: Option<UserInfo>,
    pub update_interval_hours: Option<u32>,
    pub title: Option<String>,
}

/// Parses `upload=1; download=2; total=3; expire=4`.
pub fn parse_userinfo(header: &str) -> Option<UserInfo> {
    let mut info = UserInfo::default();
    let mut seen = false;
    for part in header.split(';') {
        let Some((k, v)) = part.split_once('=') else { continue };
        let v = v.trim();
        let num = v.parse::<f64>().ok().map(|n| n.max(0.0) as u64);
        match k.trim().to_ascii_lowercase().as_str() {
            "upload" => info.upload = num.unwrap_or(0),
            "download" => info.download = num.unwrap_or(0),
            "total" => info.total = num.unwrap_or(0),
            "expire" => info.expire = num.filter(|n| *n > 0).map(|n| n as i64),
            _ => continue,
        }
        seen = true;
    }
    seen.then_some(info)
}

/// `profile-title` may be plain text or `base64:<...>`.
pub fn parse_title(header: &str) -> Option<String> {
    let header = header.trim();
    let title = match header.strip_prefix("base64:") {
        Some(b64) => String::from_utf8(decode_base64_lenient(b64)?).ok()?,
        None => header.to_string(),
    };
    Some(title.trim().to_string()).filter(|t| !t.is_empty())
}

/// Turns a subscription body into share links. The body is either base64 of a
/// newline-separated list, or the list itself.
pub fn extract_links(body: &str) -> Vec<String> {
    let body = body.trim_start_matches('\u{feff}').trim();
    let text = if body.contains("://") {
        body.to_string()
    } else {
        decode_base64_lenient(body)
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default()
    };
    text.lines()
        .map(str::trim)
        .filter(|l| l.contains("://"))
        .map(String::from)
        .collect()
}

pub fn profiles_from_links(links: &[String], subscription_id: Option<&str>) -> (Vec<Profile>, usize) {
    let mut profiles = Vec::new();
    let mut skipped = 0;
    for link in links {
        match parse_link(link) {
            Ok(p) => profiles.push(Profile {
                id: new_id(),
                name: p.name,
                subscription_id: subscription_id.map(String::from),
                link: link.clone(),
                server: p.server,
            }),
            Err(_) => skipped += 1,
        }
    }
    (profiles, skipped)
}

pub async fn fetch(client: &reqwest::Client, url: &str, subscription_id: &str) -> anyhow::Result<Fetched> {
    let resp = client.get(url).send().await?.error_for_status()?;
    let headers = resp.headers().clone();
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(String::from);

    let body = resp.text().await?;
    let links = extract_links(&body);
    if links.is_empty() {
        anyhow::bail!("subscription contains no links");
    }
    let (profiles, skipped) = profiles_from_links(&links, Some(subscription_id));
    Ok(Fetched {
        profiles,
        skipped,
        userinfo: header("subscription-userinfo").as_deref().and_then(parse_userinfo),
        update_interval_hours: header("profile-update-interval").and_then(|v| v.trim().parse().ok()),
        title: header("profile-title").as_deref().and_then(parse_title),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};

    #[test]
    fn userinfo_header() {
        let u = parse_userinfo("upload=1024; download=2048; total=10737418240; expire=1798761600").unwrap();
        assert_eq!(u.upload, 1024);
        assert_eq!(u.download, 2048);
        assert_eq!(u.total, 10737418240);
        assert_eq!(u.expire, Some(1798761600));
        assert_eq!(parse_userinfo("upload=0; download=0; total=0; expire=0").unwrap().expire, None);
        assert_eq!(parse_userinfo("garbage"), None);
    }

    #[test]
    fn title_header() {
        assert_eq!(parse_title(&format!("base64:{}", STANDARD.encode("Dogfather VPN"))).as_deref(), Some("Dogfather VPN"));
        assert_eq!(parse_title("Plain").as_deref(), Some("Plain"));
    }

    #[test]
    fn base64_body() {
        let list = "vless://a@h1:443?security=none#one\r\nvless://b@h2:443?security=none#two\n\n";
        let links = extract_links(&STANDARD.encode(list));
        assert_eq!(links.len(), 2);
        let (profiles, skipped) = profiles_from_links(&links, Some("sub"));
        assert_eq!(skipped, 0);
        assert_eq!(profiles[1].name, "two");
        assert_eq!(profiles[0].subscription_id.as_deref(), Some("sub"));
    }

    #[test]
    fn plain_body_with_unsupported() {
        let links = extract_links("vless://a@h1:443?security=none#one\nhysteria2://x@h:1#h\n");
        let (profiles, skipped) = profiles_from_links(&links, None);
        assert_eq!((profiles.len(), skipped), (1, 1));
    }
}
