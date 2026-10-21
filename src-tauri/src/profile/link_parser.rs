//! Share-link parsing: vless://, vmess://, trojan://, ss://.

use std::collections::HashMap;

use base64::alphabet;
use base64::engine::{DecodePaddingMode, Engine, GeneralPurpose, GeneralPurposeConfig};
use percent_encoding::percent_decode_str;
use serde_json::Value;

use super::{Protocol, Server};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum LinkError {
    #[error("unsupported link scheme: {0}")]
    UnsupportedScheme(String),
    #[error("unsupported transport: {0}")]
    UnsupportedTransport(String),
    #[error("malformed link: {0}")]
    Malformed(&'static str),
}

pub struct ParsedLink {
    pub name: String,
    pub server: Server,
}

const LENIENT: GeneralPurposeConfig = GeneralPurposeConfig::new()
    .with_decode_padding_mode(DecodePaddingMode::Indifferent)
    .with_decode_allow_trailing_bits(true);
const B64_STD: GeneralPurpose = GeneralPurpose::new(&alphabet::STANDARD, LENIENT);
const B64_URL: GeneralPurpose = GeneralPurpose::new(&alphabet::URL_SAFE, LENIENT);

/// Decodes base64 in either alphabet, with or without padding, ignoring whitespace.
pub fn decode_base64_lenient(input: &str) -> Option<Vec<u8>> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    B64_STD.decode(&cleaned).or_else(|_| B64_URL.decode(&cleaned)).ok()
}

fn pct(s: &str) -> String {
    percent_decode_str(s).decode_utf8_lossy().into_owned()
}

fn non_empty(s: Option<&String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Splits `scheme://userinfo@host:port?query#fragment` without the WHATWG quirks of
/// non-special schemes. Query values are percent-decoded but `+` is kept as-is.
struct RawLink<'a> {
    userinfo: Option<&'a str>,
    host: String,
    port: Option<u16>,
    query: HashMap<String, String>,
    fragment: String,
}

fn split_link(rest: &str) -> Result<RawLink<'_>, LinkError> {
    let (rest, fragment) = match rest.split_once('#') {
        Some((r, f)) => (r, pct(f)),
        None => (rest, String::new()),
    };
    let (authority, query_str) = match rest.split_once('?') {
        Some((a, q)) => (a, q),
        None => (rest, ""),
    };
    let authority = authority.trim_end_matches('/');
    let (userinfo, hostport) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };

    let (host, port) = if let Some(stripped) = hostport.strip_prefix('[') {
        let end = stripped.find(']').ok_or(LinkError::Malformed("unterminated IPv6 address"))?;
        let host = stripped[..end].to_string();
        let port = stripped[end + 1..].strip_prefix(':').map(|p| p.parse::<u16>());
        (host, port)
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), Some(p.parse::<u16>())),
            None => (hostport.to_string(), None),
        }
    };
    let port = match port {
        Some(Ok(p)) => Some(p),
        Some(Err(_)) => return Err(LinkError::Malformed("invalid port")),
        None => None,
    };

    let mut query = HashMap::new();
    for pair in query_str.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        query.insert(pct(k), pct(v));
    }

    Ok(RawLink { userinfo, host: pct(&host), port, query, fragment })
}

pub fn parse_link(link: &str) -> Result<ParsedLink, LinkError> {
    let link = link.trim();
    let (scheme, rest) = link.split_once("://").ok_or(LinkError::Malformed("missing scheme"))?;
    match scheme.to_ascii_lowercase().as_str() {
        "vless" => parse_vless(rest),
        "vmess" => parse_vmess(rest),
        "trojan" => parse_trojan(rest),
        "ss" => parse_shadowsocks(rest),
        other => Err(LinkError::UnsupportedScheme(other.to_string())),
    }
}

fn normalize_network(net: Option<&str>) -> Result<String, LinkError> {
    let net = net.unwrap_or("tcp").trim().to_ascii_lowercase();
    match net.as_str() {
        "" | "tcp" | "raw" => Ok("raw".into()),
        "xhttp" | "splithttp" => Ok("xhttp".into()),
        "ws" | "websocket" => Ok("ws".into()),
        "grpc" | "gun" => Ok("grpc".into()),
        "httpupgrade" => Ok("httpupgrade".into()),
        _ => Err(LinkError::UnsupportedTransport(net)),
    }
}

/// Applies the transport/security query parameters shared by vless:// and trojan:// links.
fn apply_stream_params(server: &mut Server, q: &HashMap<String, String>, default_security: &str) -> Result<(), LinkError> {
    server.network = normalize_network(q.get("type").map(String::as_str))?;
    server.header_type = non_empty(q.get("headerType")).filter(|h| h != "none");
    server.host = non_empty(q.get("host"));
    server.path = non_empty(q.get("path"));
    server.service_name = non_empty(q.get("serviceName"));
    server.authority = non_empty(q.get("authority"));
    server.mode = non_empty(q.get("mode"));
    if let Some(extra) = non_empty(q.get("extra")) {
        server.extra = serde_json::from_str::<Value>(&extra).ok();
    }

    server.security = non_empty(q.get("security")).unwrap_or_else(|| default_security.to_string()).to_ascii_lowercase();
    if !matches!(server.security.as_str(), "none" | "tls" | "reality") {
        return Err(LinkError::Malformed("unknown security"));
    }
    server.sni = non_empty(q.get("sni")).or_else(|| non_empty(q.get("peer")));
    server.fingerprint = non_empty(q.get("fp"));
    server.alpn = q
        .get("alpn")
        .map(|a| a.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    server.allow_insecure = matches!(q.get("allowInsecure").or(q.get("insecure")).map(String::as_str), Some("1" | "true"));
    server.public_key = non_empty(q.get("pbk"));
    server.short_id = non_empty(q.get("sid"));
    server.spider_x = non_empty(q.get("spx"));
    server.mldsa65_verify = non_empty(q.get("pqv"));
    if server.security == "reality" && server.public_key.is_none() {
        return Err(LinkError::Malformed("REALITY link without public key (pbk)"));
    }
    Ok(())
}

fn require_host_port(raw: &RawLink) -> Result<(String, u16), LinkError> {
    if raw.host.is_empty() {
        return Err(LinkError::Malformed("missing host"));
    }
    let port = raw.port.ok_or(LinkError::Malformed("missing port"))?;
    Ok((raw.host.clone(), port))
}

fn display_name(fragment: String, address: &str, port: u16) -> String {
    let name = fragment.trim();
    if name.is_empty() { format!("{address}:{port}") } else { name.to_string() }
}

fn parse_vless(rest: &str) -> Result<ParsedLink, LinkError> {
    let raw = split_link(rest)?;
    let (address, port) = require_host_port(&raw)?;
    let id = raw.userinfo.map(pct).filter(|s| !s.is_empty()).ok_or(LinkError::Malformed("missing user id"))?;
    let mut server = Server {
        protocol: Some(Protocol::Vless),
        address,
        port,
        credential: id,
        flow: non_empty(raw.query.get("flow")),
        encryption: non_empty(raw.query.get("encryption")),
        ..Default::default()
    };
    apply_stream_params(&mut server, &raw.query, "none")?;
    let name = display_name(raw.fragment, &server.address, port);
    Ok(ParsedLink { name, server })
}

fn parse_trojan(rest: &str) -> Result<ParsedLink, LinkError> {
    let raw = split_link(rest)?;
    let (address, port) = require_host_port(&raw)?;
    let password = raw.userinfo.map(pct).filter(|s| !s.is_empty()).ok_or(LinkError::Malformed("missing password"))?;
    let mut server = Server {
        protocol: Some(Protocol::Trojan),
        address,
        port,
        credential: password,
        flow: non_empty(raw.query.get("flow")),
        ..Default::default()
    };
    apply_stream_params(&mut server, &raw.query, "tls")?;
    let name = display_name(raw.fragment, &server.address, port);
    Ok(ParsedLink { name, server })
}

/// Reads a JSON value that may be a string or a number (v2rayN writes both).
fn json_str(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn parse_vmess(rest: &str) -> Result<ParsedLink, LinkError> {
    let body = rest.split('#').next().unwrap_or(rest);
    let bytes = decode_base64_lenient(body).ok_or(LinkError::Malformed("vmess payload is not base64"))?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|_| LinkError::Malformed("vmess payload is not JSON"))?;

    let address = json_str(&v, "add").ok_or(LinkError::Malformed("missing host"))?;
    let port = json_str(&v, "port")
        .and_then(|p| p.parse::<u16>().ok())
        .ok_or(LinkError::Malformed("missing port"))?;
    let id = json_str(&v, "id").ok_or(LinkError::Malformed("missing user id"))?;
    let network = normalize_network(json_str(&v, "net").as_deref())?;
    let tls = json_str(&v, "tls").unwrap_or_default().to_ascii_lowercase();

    let mut server = Server {
        protocol: Some(Protocol::Vmess),
        address,
        port,
        credential: id,
        alter_id: json_str(&v, "aid").and_then(|a| a.parse().ok()).filter(|a| *a > 0),
        method: json_str(&v, "scy").filter(|s| s != "auto"),
        header_type: json_str(&v, "type").filter(|h| h != "none"),
        host: json_str(&v, "host"),
        path: json_str(&v, "path"),
        security: if tls == "tls" { "tls".into() } else { "none".into() },
        sni: json_str(&v, "sni"),
        fingerprint: json_str(&v, "fp"),
        alpn: json_str(&v, "alpn")
            .map(|a| a.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default(),
        allow_insecure: matches!(json_str(&v, "allowInsecure").as_deref(), Some("1" | "true")),
        ..Default::default()
    };
    if network == "grpc" {
        // v2rayN stores the gRPC service name in "path" and the mode in "type".
        server.service_name = server.path.take();
        server.mode = server.header_type.take();
    }
    if network == "xhttp" {
        server.mode = server.header_type.take();
    }
    server.network = network;
    let name = display_name(json_str(&v, "ps").unwrap_or_default(), &server.address, port);
    Ok(ParsedLink { name, server })
}

fn parse_shadowsocks(rest: &str) -> Result<ParsedLink, LinkError> {
    let (body, fragment) = match rest.split_once('#') {
        Some((b, f)) => (b, pct(f)),
        None => (rest, String::new()),
    };
    let body = body.split('?').next().unwrap_or(body).trim_end_matches('/');

    // Legacy form: ss://BASE64(method:password@host:port)
    let expanded;
    let body = if !body.contains('@') {
        let bytes = decode_base64_lenient(body).ok_or(LinkError::Malformed("ss payload is not base64"))?;
        expanded = String::from_utf8(bytes).map_err(|_| LinkError::Malformed("ss payload is not UTF-8"))?;
        expanded.as_str()
    } else {
        body
    };

    let raw = split_link(body)?;
    let (address, port) = require_host_port(&raw)?;
    let userinfo = raw.userinfo.ok_or(LinkError::Malformed("missing method and password"))?;
    // SIP002 allows userinfo to be base64(method:password) or percent-encoded plain text.
    let plain = pct(userinfo);
    let creds = if plain.contains(':') {
        plain
    } else {
        let bytes = decode_base64_lenient(userinfo).ok_or(LinkError::Malformed("bad ss userinfo"))?;
        String::from_utf8(bytes).map_err(|_| LinkError::Malformed("bad ss userinfo"))?
    };
    let (method, password) = creds.split_once(':').ok_or(LinkError::Malformed("bad ss userinfo"))?;

    let server = Server {
        protocol: Some(Protocol::Shadowsocks),
        address,
        port,
        credential: password.to_string(),
        method: Some(method.to_ascii_lowercase()),
        network: "raw".into(),
        security: "none".into(),
        ..Default::default()
    };
    let name = display_name(fragment, &server.address, port);
    Ok(ParsedLink { name, server })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;

    // Shapes match what 3X-UI 3.8 generates for the Dogfather inbounds; ids and keys are fake.
    const REALITY_TCP: &str = "vless://11111111-2222-3333-4444-555555555555@85.137.172.26:443?type=tcp&encryption=none&security=reality&pbk=Z84J2IelR9ch3k8VtlVhhs5ycBUlXA7wHBWcBrjqnAw&fp=chrome&sni=intel.com&sid=6ba85179e30d4fc2&spx=%2F&flow=xtls-rprx-vision#%D0%9F%D1%80%D1%8F%D0%BC%D0%BE%D0%B9-%D0%A8%D0%B2%D0%B5%D1%86%D0%B8%D1%8F";
    const REALITY_XHTTP: &str = "vless://11111111-2222-3333-4444-555555555555@77.73.66.190:443?type=xhttp&encryption=none&path=%2Fapi&host=&mode=auto&extra=%7B%22xPaddingBytes%22%3A%22100-1000%22%7D&security=reality&pbk=Z84J2IelR9ch3k8VtlVhhs5ycBUlXA7wHBWcBrjqnAw&fp=firefox&sni=reddit.com&sid=ab&spx=%2F#Cascade";

    #[test]
    fn vless_reality_tcp() {
        let p = parse_link(REALITY_TCP).unwrap();
        assert_eq!(p.name, "Прямой-Швеция");
        let s = p.server;
        assert_eq!(s.protocol, Some(Protocol::Vless));
        assert_eq!((s.address.as_str(), s.port), ("85.137.172.26", 443));
        assert_eq!(s.network, "raw");
        assert_eq!(s.security, "reality");
        assert_eq!(s.flow.as_deref(), Some("xtls-rprx-vision"));
        assert_eq!(s.sni.as_deref(), Some("intel.com"));
        assert_eq!(s.short_id.as_deref(), Some("6ba85179e30d4fc2"));
        assert_eq!(s.spider_x.as_deref(), Some("/"));
        assert_eq!(s.encryption.as_deref(), Some("none"));
    }

    #[test]
    fn vless_reality_xhttp_with_extra() {
        let s = parse_link(REALITY_XHTTP).unwrap().server;
        assert_eq!(s.network, "xhttp");
        assert_eq!(s.path.as_deref(), Some("/api"));
        assert_eq!(s.host, None);
        assert_eq!(s.mode.as_deref(), Some("auto"));
        assert_eq!(s.extra, Some(serde_json::json!({"xPaddingBytes": "100-1000"})));
        assert_eq!(s.fingerprint.as_deref(), Some("firefox"));
    }

    #[test]
    fn vless_ws_tls_ipv6() {
        let s = parse_link("vless://id@[2001:db8::1]:8443?type=ws&security=tls&path=%2Fws%3Fed%3D2048&host=cdn.example.com&sni=cdn.example.com&alpn=h2%2Chttp%2F1.1").unwrap().server;
        assert_eq!(s.address, "2001:db8::1");
        assert_eq!(s.port, 8443);
        assert_eq!(s.path.as_deref(), Some("/ws?ed=2048"));
        assert_eq!(s.alpn, vec!["h2", "http/1.1"]);
    }

    #[test]
    fn vless_without_name_uses_address() {
        let p = parse_link("vless://id@example.com:443?security=none").unwrap();
        assert_eq!(p.name, "example.com:443");
        assert_eq!(p.server.network, "raw");
    }

    #[test]
    fn reality_requires_public_key() {
        assert!(parse_link("vless://id@h:443?security=reality&sni=a.com").is_err());
    }

    #[test]
    fn vmess_v2rayn_json() {
        let json = r#"{"v":"2","ps":"VMess WS","add":"vm.example.com","port":"443","id":"abc","aid":"0","scy":"auto","net":"ws","type":"none","host":"vm.example.com","path":"/vm","tls":"tls","sni":"vm.example.com","fp":"chrome"}"#;
        let link = format!("vmess://{}", STANDARD.encode(json));
        let p = parse_link(&link).unwrap();
        assert_eq!(p.name, "VMess WS");
        let s = p.server;
        assert_eq!(s.protocol, Some(Protocol::Vmess));
        assert_eq!(s.port, 443);
        assert_eq!(s.alter_id, None);
        assert_eq!(s.method, None);
        assert_eq!(s.network, "ws");
        assert_eq!(s.security, "tls");
        assert_eq!(s.header_type, None);
    }

    #[test]
    fn vmess_grpc_numeric_port() {
        let json = r#"{"ps":"g","add":"1.2.3.4","port":2053,"id":"abc","net":"grpc","type":"multi","path":"svc","tls":""}"#;
        let s = parse_link(&format!("vmess://{}", STANDARD.encode(json))).unwrap().server;
        assert_eq!(s.port, 2053);
        assert_eq!(s.service_name.as_deref(), Some("svc"));
        assert_eq!(s.mode.as_deref(), Some("multi"));
        assert_eq!(s.security, "none");
    }

    #[test]
    fn trojan_defaults_to_tls() {
        let s = parse_link("trojan://p%40ss@tr.example.com:443?sni=tr.example.com#Trojan").unwrap().server;
        assert_eq!(s.protocol, Some(Protocol::Trojan));
        assert_eq!(s.credential, "p@ss");
        assert_eq!(s.security, "tls");
    }

    #[test]
    fn shadowsocks_sip002_base64_userinfo() {
        let ui = STANDARD.encode("chacha20-ietf-poly1305:secret");
        let s = parse_link(&format!("ss://{ui}@ss.example.com:8388#SS")).unwrap().server;
        assert_eq!(s.method.as_deref(), Some("chacha20-ietf-poly1305"));
        assert_eq!(s.credential, "secret");
        assert_eq!(s.port, 8388);
    }

    #[test]
    fn shadowsocks_2022_plain_userinfo() {
        let s = parse_link("ss://2022-blake3-aes-128-gcm:YctPZ6U7xPPcU%2Bgp3u%2B0tx%2FtRizJN9K8y%2BuKlW2qjlI%3D@1.2.3.4:443#x").unwrap().server;
        assert_eq!(s.method.as_deref(), Some("2022-blake3-aes-128-gcm"));
        assert_eq!(s.credential, "YctPZ6U7xPPcU+gp3u+0tx/tRizJN9K8y+uKlW2qjlI=");
    }

    #[test]
    fn shadowsocks_legacy_full_base64() {
        let body = STANDARD.encode("aes-256-gcm:pw@5.6.7.8:8000");
        let p = parse_link(&format!("ss://{body}#Old")).unwrap();
        assert_eq!(p.server.address, "5.6.7.8");
        assert_eq!(p.server.credential, "pw");
        assert_eq!(p.name, "Old");
    }

    #[test]
    fn unsupported_scheme() {
        assert_eq!(
            parse_link("hysteria2://pw@h:443").err(),
            Some(LinkError::UnsupportedScheme("hysteria2".into()))
        );
    }
}

#[cfg(test)]
mod dump {
    /// Prints the parsed server of `$LITERAY_LINK` as JSON (tooling for manual end-to-end runs):
    /// `cargo test dump_link -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn dump_link() {
        let Ok(link) = std::env::var("LITERAY_LINK") else {
            eprintln!("LITERAY_LINK not set, nothing to dump");
            return;
        };
        let p = super::parse_link(&link).unwrap();
        println!("LINKJSON {}", serde_json::json!({"name": p.name, "server": p.server}));
    }
}
