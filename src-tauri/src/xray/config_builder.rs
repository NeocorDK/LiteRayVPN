//! Builds the Xray JSON config from a server, the connection mode and split-tunneling rules.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::profile::{Protocol, Server};
use crate::routing::{ListMode, RoutingSettings, RU_DOMAINS, RU_IPS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ConnMode {
    #[default]
    Tun,
    SystemProxy,
}

pub struct BuildOptions<'a> {
    pub server: &'a Server,
    pub mode: ConnMode,
    pub routing: &'a RoutingSettings,
    pub mixed_port: u16,
    pub metrics_port: u16,
    pub ipv6: bool,
    /// Our own executable path (forward slashes), routed direct to avoid loops.
    pub self_exe: Option<String>,
    /// Name of the wintun adapter (see `tun_adapter_name`).
    pub tun_name: &'a str,
}

/// Alternates between two adapter names: right after an unclean stop Windows is still
/// removing the old adapter, and creating one with the same name fails for a few seconds.
pub fn tun_adapter_name(slot: u8) -> &'static str {
    if slot % 2 == 0 { "LiteRay" } else { "LiteRay-2" }
}

pub const TAG_PROXY: &str = "proxy";
pub const TAG_DIRECT: &str = "direct";
pub const TAG_BLOCK: &str = "block";
const TAG_DNS_OUT: &str = "dns-out";
const TAG_TUN_IN: &str = "tun-in";
const TAG_MIXED_IN: &str = "mixed-in";
const TAG_DNS_INTERNAL: &str = "dns-internal";

const REMOTE_DNS: &str = "https://1.1.1.1/dns-query";
const DIRECT_DNS: &str = "77.88.8.8";
const TUN_DNS: &str = "172.19.0.2";

pub fn build(opts: &BuildOptions) -> anyhow::Result<Value> {
    let r = opts.routing;
    let user_target = match r.list_mode {
        ListMode::Exclude => TAG_DIRECT,
        ListMode::Include => TAG_PROXY,
    };
    let final_target = match r.list_mode {
        ListMode::Exclude => TAG_PROXY,
        ListMode::Include => TAG_DIRECT,
    };

    let sniffing = json!({"enabled": true, "destOverride": ["http", "tls", "quic"], "routeOnly": true});
    let mut inbounds = vec![json!({
        "tag": TAG_MIXED_IN,
        "protocol": "mixed",
        "listen": "127.0.0.1",
        "port": opts.mixed_port,
        "settings": {"udp": true},
        "sniffing": sniffing,
    })];
    if opts.mode == ConnMode::Tun {
        inbounds.insert(0, json!({
            "tag": TAG_TUN_IN,
            "protocol": "tun",
            "settings": {
                "name": opts.tun_name,
                "mtu": 1500,
                "gateway": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"],
                "dns": [TUN_DNS],
                "autoSystemRoutingTable": ["0.0.0.0/0", "::/0"],
                "autoOutboundsInterface": "auto",
            },
            "sniffing": sniffing,
        }));
    }

    let mut rules: Vec<Value> = Vec::new();
    if opts.mode == ConnMode::Tun {
        // Traffic of the core and of the client itself must never loop back into the tunnel.
        let mut own = vec![json!("self/"), json!("xray/")];
        if let Some(exe) = &opts.self_exe {
            own.push(json!(exe));
        }
        rules.push(json!({"inboundTag": [TAG_TUN_IN], "process": own, "outboundTag": TAG_DIRECT}));
        rules.push(json!({"inboundTag": [TAG_TUN_IN], "port": 53, "outboundTag": TAG_DNS_OUT}));
    }
    // Xray's own DNS: the domestic resolver goes direct, the remote one through the proxy.
    rules.push(json!({"inboundTag": [TAG_DNS_INTERNAL], "ip": [DIRECT_DNS], "outboundTag": TAG_DIRECT}));
    rules.push(json!({"inboundTag": [TAG_DNS_INTERNAL], "outboundTag": TAG_PROXY}));
    if r.lan_direct {
        rules.push(json!({"ip": ["geoip:private"], "outboundTag": TAG_DIRECT}));
        rules.push(json!({"domain": ["full:localhost"], "outboundTag": TAG_DIRECT}));
    }
    if !opts.ipv6 && opts.mode == ConnMode::Tun {
        rules.push(json!({"inboundTag": [TAG_TUN_IN], "ip": ["::/0"], "outboundTag": TAG_BLOCK}));
    }
    if r.block_ads {
        rules.push(json!({"domain": ["geosite:category-ads-all"], "outboundTag": TAG_BLOCK}));
    }
    if !r.apps.is_empty() {
        rules.push(json!({"process": r.apps, "outboundTag": user_target}));
    }
    if !r.domains.is_empty() {
        rules.push(json!({"domain": r.domains, "outboundTag": user_target}));
    }
    if !r.ips.is_empty() {
        rules.push(json!({"ip": r.ips, "outboundTag": user_target}));
    }
    if r.ru_direct {
        rules.push(json!({"domain": RU_DOMAINS, "outboundTag": TAG_DIRECT}));
        rules.push(json!({"ip": RU_IPS, "outboundTag": TAG_DIRECT}));
    }
    rules.push(json!({"network": "tcp,udp", "outboundTag": final_target}));

    // Domains resolved by the domestic DNS: whatever goes direct by domain, plus the
    // server's own hostname (resolving it through the proxy would be circular).
    let mut direct_dns_domains: Vec<String> = Vec::new();
    if opts.server.address.parse::<std::net::IpAddr>().is_err() {
        direct_dns_domains.push(format!("full:{}", opts.server.address));
    }
    if r.ru_direct {
        direct_dns_domains.extend(RU_DOMAINS.iter().map(|d| d.to_string()));
    }
    if r.list_mode == ListMode::Exclude {
        direct_dns_domains.extend(r.domains.iter().cloned());
    }

    let mut proxy = proxy_outbound(opts.server)?;
    if !opts.ipv6 {
        // In proxy mode apps hand over domain names; left as-is, the server resolves them and may
        // pick IPv6, which some setups (e.g. IPv4-only cascades) leak past the tunnel. Resolve to
        // IPv4 here, through our own DNS, so both modes behave the same as TUN.
        proxy["targetStrategy"] = json!("UseIPv4");
    }

    let config = json!({
        // Per-connection access lines would grow the log without bound in long sessions.
        "log": {"loglevel": "warning", "access": "none"},
        "stats": {},
        "metrics": {"tag": "metrics", "listen": format!("127.0.0.1:{}", opts.metrics_port)},
        "policy": {
            "system": {"statsOutboundUplink": true, "statsOutboundDownlink": true},
        },
        "dns": {
            "tag": TAG_DNS_INTERNAL,
            "servers": [
                {"address": DIRECT_DNS, "domains": direct_dns_domains, "skipFallback": true},
                REMOTE_DNS,
            ],
            "queryStrategy": if opts.ipv6 { "UseIP" } else { "UseIPv4" },
        },
        "inbounds": inbounds,
        "outbounds": [
            proxy,
            {
                "tag": TAG_DIRECT,
                "protocol": "freedom",
                "streamSettings": {"sockopt": {"domainStrategy": if opts.ipv6 { "UseIP" } else { "UseIPv4" }}},
            },
            {"tag": TAG_BLOCK, "protocol": "blackhole"},
            {"tag": TAG_DNS_OUT, "protocol": "dns"},
        ],
        "routing": {"domainStrategy": "IPIfNonMatch", "rules": rules},
        "burstObservatory": {
            "subjectSelector": [TAG_PROXY],
            "pingConfig": {
                "destination": "https://www.gstatic.com/generate_204",
                "interval": "10s",
                "timeout": "5s",
                "sampling": 2,
            },
        },
    });
    Ok(config)
}

fn insert_opt(map: &mut Map<String, Value>, key: &str, value: &Option<String>) {
    if let Some(v) = value {
        map.insert(key.into(), json!(v));
    }
}

pub fn proxy_outbound(s: &Server) -> anyhow::Result<Value> {
    let protocol = s.protocol.ok_or_else(|| anyhow::anyhow!("server protocol is not set"))?;
    let settings = match protocol {
        Protocol::Vless => {
            let mut user = Map::new();
            user.insert("id".into(), json!(s.credential));
            user.insert("encryption".into(), json!(s.encryption.as_deref().unwrap_or("none")));
            insert_opt(&mut user, "flow", &s.flow);
            json!({"vnext": [{"address": s.address, "port": s.port, "users": [user]}]})
        }
        Protocol::Vmess => {
            let mut user = Map::new();
            user.insert("id".into(), json!(s.credential));
            user.insert("security".into(), json!(s.method.as_deref().unwrap_or("auto")));
            if let Some(aid) = s.alter_id {
                user.insert("alterId".into(), json!(aid));
            }
            json!({"vnext": [{"address": s.address, "port": s.port, "users": [user]}]})
        }
        Protocol::Trojan => {
            let mut srv = Map::new();
            srv.insert("address".into(), json!(s.address));
            srv.insert("port".into(), json!(s.port));
            srv.insert("password".into(), json!(s.credential));
            insert_opt(&mut srv, "flow", &s.flow);
            json!({"servers": [srv]})
        }
        Protocol::Shadowsocks => json!({"servers": [{
            "address": s.address,
            "port": s.port,
            "method": s.method.as_deref().unwrap_or("aes-256-gcm"),
            "password": s.credential,
        }]}),
    };
    let protocol_name = match protocol {
        Protocol::Vless => "vless",
        Protocol::Vmess => "vmess",
        Protocol::Trojan => "trojan",
        Protocol::Shadowsocks => "shadowsocks",
    };
    Ok(json!({
        "tag": TAG_PROXY,
        "protocol": protocol_name,
        "settings": settings,
        "streamSettings": stream_settings(s)?,
    }))
}

fn stream_settings(s: &Server) -> anyhow::Result<Value> {
    let mut stream = Map::new();
    let network = if s.network.is_empty() { "raw" } else { s.network.as_str() };
    stream.insert("network".into(), json!(network));

    match network {
        "raw" => {
            if s.header_type.as_deref() == Some("http") {
                let mut request = Map::new();
                request.insert("path".into(), json!([s.path.as_deref().unwrap_or("/")]));
                if let Some(host) = &s.host {
                    let hosts: Vec<&str> = host.split(',').map(str::trim).collect();
                    request.insert("headers".into(), json!({"Host": hosts}));
                }
                stream.insert("rawSettings".into(), json!({"header": {"type": "http", "request": request}}));
            }
        }
        "ws" | "httpupgrade" => {
            let mut ws = Map::new();
            ws.insert("path".into(), json!(s.path.as_deref().unwrap_or("/")));
            insert_opt(&mut ws, "host", &s.host);
            let key = if network == "ws" { "wsSettings" } else { "httpupgradeSettings" };
            stream.insert(key.into(), Value::Object(ws));
        }
        "grpc" => {
            let mut grpc = Map::new();
            grpc.insert("serviceName".into(), json!(s.service_name.as_deref().unwrap_or("")));
            insert_opt(&mut grpc, "authority", &s.authority);
            if s.mode.as_deref() == Some("multi") {
                grpc.insert("multiMode".into(), json!(true));
            }
            stream.insert("grpcSettings".into(), Value::Object(grpc));
        }
        "xhttp" => {
            let mut xhttp = Map::new();
            xhttp.insert("path".into(), json!(s.path.as_deref().unwrap_or("/")));
            insert_opt(&mut xhttp, "host", &s.host);
            xhttp.insert("mode".into(), json!(s.mode.as_deref().unwrap_or("auto")));
            if let Some(extra) = &s.extra {
                xhttp.insert("extra".into(), extra.clone());
            }
            stream.insert("xhttpSettings".into(), Value::Object(xhttp));
        }
        other => anyhow::bail!("unsupported transport: {other}"),
    }

    let security = if s.security.is_empty() { "none" } else { s.security.as_str() };
    stream.insert("security".into(), json!(security));
    match security {
        "none" => {}
        "tls" => {
            let mut tls = Map::new();
            let sni = s.sni.clone().or_else(|| s.host.clone()).unwrap_or_else(|| s.address.clone());
            tls.insert("serverName".into(), json!(sni));
            tls.insert("fingerprint".into(), json!(s.fingerprint.as_deref().unwrap_or("chrome")));
            if !s.alpn.is_empty() {
                tls.insert("alpn".into(), json!(s.alpn));
            }
            if s.allow_insecure {
                tls.insert("allowInsecure".into(), json!(true));
            }
            stream.insert("tlsSettings".into(), Value::Object(tls));
        }
        "reality" => {
            let mut reality = Map::new();
            reality.insert("serverName".into(), json!(s.sni.as_deref().unwrap_or("")));
            reality.insert("fingerprint".into(), json!(s.fingerprint.as_deref().unwrap_or("chrome")));
            reality.insert("publicKey".into(), json!(s.public_key.as_deref().unwrap_or("")));
            reality.insert("shortId".into(), json!(s.short_id.as_deref().unwrap_or("")));
            insert_opt(&mut reality, "spiderX", &s.spider_x);
            insert_opt(&mut reality, "mldsa65Verify", &s.mldsa65_verify);
            stream.insert("realitySettings".into(), Value::Object(reality));
        }
        other => anyhow::bail!("unsupported security: {other}"),
    }
    Ok(Value::Object(stream))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::link_parser::parse_link;
    use crate::routing::RoutingSettings;

    const LINKS: &[(&str, &str)] = &[
        ("vless_reality_tcp", "vless://11111111-2222-3333-4444-555555555555@85.137.172.26:443?type=tcp&encryption=none&security=reality&pbk=Z84J2IelR9ch3k8VtlVhhs5ycBUlXA7wHBWcBrjqnAw&fp=chrome&sni=intel.com&sid=6ba85179e30d4fc2&spx=%2F&flow=xtls-rprx-vision#SE"),
        ("vless_reality_xhttp", "vless://11111111-2222-3333-4444-555555555555@77.73.66.190:443?type=xhttp&encryption=none&path=%2Fapi&mode=auto&extra=%7B%22xPaddingBytes%22%3A%22100-1000%22%7D&security=reality&pbk=Z84J2IelR9ch3k8VtlVhhs5ycBUlXA7wHBWcBrjqnAw&fp=firefox&sni=reddit.com&sid=ab#RU"),
        ("vless_ws_tls_domain", "vless://11111111-2222-3333-4444-555555555555@cdn.example.com:443?type=ws&security=tls&path=%2Fws&host=cdn.example.com&sni=cdn.example.com#WS"),
        ("vless_grpc_tls", "vless://11111111-2222-3333-4444-555555555555@g.example.com:443?type=grpc&security=tls&serviceName=svc&mode=multi#G"),
        ("vless_httpupgrade", "vless://11111111-2222-3333-4444-555555555555@h.example.com:443?type=httpupgrade&security=tls&sni=h.example.com&path=%2Fu&host=h.example.com#HU"),
        // Xray refuses unencrypted VLESS to public addresses, so this one uses a private IP.
        ("vless_tcp_http_header", "vless://11111111-2222-3333-4444-555555555555@10.0.0.5:80?type=tcp&headerType=http&host=a.com,b.com&path=%2F#H"),
        ("trojan_tls", "trojan://secret@tr.example.com:443?security=tls&sni=tr.example.com&alpn=h2%2Chttp%2F1.1#T"),
        ("ss_2022", "ss://2022-blake3-aes-128-gcm:YctPZ6U7xPPcU%2Bgp3u%2B0tx%2FtRizJN9K8y%2BuKlW2qjlI%3D@1.2.3.4:443#SS"),
        ("vmess_ws_tls", "vmess://eyJ2IjoiMiIsInBzIjoiVk0iLCJhZGQiOiJ2bS5leGFtcGxlLmNvbSIsInBvcnQiOiI0NDMiLCJpZCI6IjExMTExMTExLTIyMjItMzMzMy00NDQ0LTU1NTU1NTU1NTU1NSIsImFpZCI6IjAiLCJzY3kiOiJhdXRvIiwibmV0Ijoid3MiLCJ0eXBlIjoibm9uZSIsImhvc3QiOiJ2bS5leGFtcGxlLmNvbSIsInBhdGgiOiIvdm0iLCJ0bHMiOiJ0bHMiLCJzbmkiOiJ2bS5leGFtcGxlLmNvbSJ9"),
    ];

    fn routing_full() -> RoutingSettings {
        RoutingSettings {
            apps: vec!["C:/Windows/System32/curl.exe".into(), "D:/Games/".into(), "Telegram".into()],
            domains: vec!["domain:example.org".into(), "geosite:youtube".into()],
            ips: vec!["1.1.1.1".into(), "10.20.0.0/16".into(), "geoip:nl".into()],
            block_ads: true,
            ..Default::default()
        }
    }

    fn opts<'a>(server: &'a Server, routing: &'a RoutingSettings, mode: ConnMode) -> BuildOptions<'a> {
        BuildOptions {
            server,
            mode,
            routing,
            mixed_port: 10808,
            metrics_port: 19090,
            ipv6: false,
            self_exe: Some("C:/Program Files/LiteRay/LiteRay.exe".into()),
            tun_name: "LiteRay",
        }
    }

    /// Runs `xray run -test` when the core has been fetched (scripts/fetch-core.ps1).
    fn xray_test(name: &str, config: &Value) {
        let xray_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/xray");
        let xray = xray_dir.join("xray.exe");
        if !xray.exists() {
            eprintln!("skipping xray -test for {name}: core not fetched");
            return;
        }
        let path = std::env::temp_dir().join(format!("literay-test-{name}-{}.json", std::process::id()));
        std::fs::write(&path, serde_json::to_vec_pretty(config).unwrap()).unwrap();
        let out = std::process::Command::new(&xray)
            .args(["run", "-test", "-c"])
            .arg(&path)
            .env("XRAY_LOCATION_ASSET", &xray_dir)
            .output()
            .unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(
            out.status.success(),
            "xray rejected config {name}:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn every_link_type_builds_a_valid_tun_config() {
        let routing = RoutingSettings::default();
        for (name, link) in LINKS {
            let server = parse_link(link).unwrap().server;
            let config = build(&opts(&server, &routing, ConnMode::Tun)).unwrap();
            insta::assert_json_snapshot!(format!("outbound_{name}"), config["outbounds"][0]);
            xray_test(name, &config);
        }
    }

    #[test]
    fn tun_exclude_mode_full_rules() {
        let server = parse_link(LINKS[1].1).unwrap().server;
        let routing = routing_full();
        let config = build(&opts(&server, &routing, ConnMode::Tun)).unwrap();
        insta::assert_json_snapshot!("tun_exclude_full", config);
        xray_test("tun_exclude_full", &config);
    }

    #[test]
    fn proxy_include_mode() {
        let server = parse_link(LINKS[2].1).unwrap().server;
        let routing = RoutingSettings { list_mode: ListMode::Include, ru_direct: false, ..routing_full() };
        let config = build(&opts(&server, &routing, ConnMode::SystemProxy)).unwrap();
        insta::assert_json_snapshot!("proxy_include", config);
        xray_test("proxy_include", &config);

        let rules = config["routing"]["rules"].as_array().unwrap();
        assert_eq!(rules.last().unwrap()["outboundTag"], TAG_DIRECT);
        assert_eq!(config["outbounds"][0]["targetStrategy"], "UseIPv4");
        assert!(rules.iter().all(|r| r.get("inboundTag") != Some(&json!([TAG_TUN_IN]))));
        assert_eq!(config["inbounds"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn server_hostname_resolves_via_direct_dns() {
        let server = parse_link(LINKS[2].1).unwrap().server;
        let routing = RoutingSettings::default();
        let config = build(&opts(&server, &routing, ConnMode::Tun)).unwrap();
        let domains = config["dns"]["servers"][0]["domains"].as_array().unwrap();
        assert!(domains.contains(&json!("full:cdn.example.com")));
    }
}
