//! Traffic and latency from Xray's metrics endpoint (`/debug/vars`).

use serde::Serialize;
use serde_json::Value;

use super::config_builder::{TAG_DIRECT, TAG_PROXY};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Counters {
    pub proxy_up: u64,
    pub proxy_down: u64,
    pub direct_up: u64,
    pub direct_down: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sample {
    pub counters: Counters,
    /// Real delay through the tunnel in ms; `None` until the first probe or when the probe fails.
    pub delay_ms: Option<u32>,
}

/// One-second snapshot pushed to the UI.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsEvent {
    pub up_speed: u64,
    pub down_speed: u64,
    pub direct_up_speed: u64,
    pub direct_down_speed: u64,
    pub total_up: u64,
    pub total_down: u64,
    pub delay_ms: Option<u32>,
}

fn counter(v: &Value, tag: &str, dir: &str) -> u64 {
    v.pointer(&format!("/stats/outbound/{tag}/{dir}")).and_then(Value::as_u64).unwrap_or(0)
}

pub fn parse_vars(v: &Value) -> Sample {
    let counters = Counters {
        proxy_up: counter(v, TAG_PROXY, "uplink"),
        proxy_down: counter(v, TAG_PROXY, "downlink"),
        direct_up: counter(v, TAG_DIRECT, "uplink"),
        direct_down: counter(v, TAG_DIRECT, "downlink"),
    };
    let obs = v.pointer(&format!("/observatory/{TAG_PROXY}"));
    let alive = obs.and_then(|o| o.get("alive")).and_then(Value::as_bool).unwrap_or(false);
    let delay_ms = obs
        .and_then(|o| o.get("delay"))
        .and_then(Value::as_u64)
        .filter(|d| alive && *d > 0 && *d < 60_000)
        .map(|d| d as u32);
    Sample { counters, delay_ms }
}

pub async fn fetch(client: &reqwest::Client, metrics_port: u16) -> anyhow::Result<Sample> {
    let v: Value = client
        .get(format!("http://127.0.0.1:{metrics_port}/debug/vars"))
        .send()
        .await?
        .json()
        .await?;
    Ok(parse_vars(&v))
}

/// Accumulates samples into per-second speeds and session totals.
#[derive(Default)]
pub struct SpeedMeter {
    last: Option<(std::time::Instant, Counters)>,
}

impl SpeedMeter {
    pub fn update(&mut self, sample: &Sample) -> StatsEvent {
        let now = std::time::Instant::now();
        let c = sample.counters;
        let mut ev = StatsEvent {
            total_up: c.proxy_up,
            total_down: c.proxy_down,
            delay_ms: sample.delay_ms,
            ..Default::default()
        };
        if let Some((t, prev)) = self.last {
            let secs = now.duration_since(t).as_secs_f64().max(0.001);
            let rate = |cur: u64, old: u64| (cur.saturating_sub(old) as f64 / secs) as u64;
            ev.up_speed = rate(c.proxy_up, prev.proxy_up);
            ev.down_speed = rate(c.proxy_down, prev.proxy_down);
            ev.direct_up_speed = rate(c.direct_up, prev.direct_up);
            ev.direct_down_speed = rate(c.direct_down, prev.direct_down);
        }
        self.last = Some((now, c));
        ev
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_expvar_shape() {
        let v = json!({
            "stats": {"outbound": {
                "proxy": {"uplink": 100, "downlink": 2000},
                "direct": {"uplink": 5, "downlink": 50}
            }},
            "observatory": {"proxy": {"alive": true, "delay": 182, "outbound_tag": "proxy"}}
        });
        let s = parse_vars(&v);
        assert_eq!(s.counters, Counters { proxy_up: 100, proxy_down: 2000, direct_up: 5, direct_down: 50 });
        assert_eq!(s.delay_ms, Some(182));
    }

    #[test]
    fn dead_probe_has_no_delay() {
        let v = json!({"observatory": {"proxy": {"alive": false, "delay": 99999999}}});
        assert_eq!(parse_vars(&v).delay_ms, None);
        assert_eq!(parse_vars(&json!({})).counters, Counters::default());
    }
}
