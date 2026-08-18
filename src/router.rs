use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpFamily {
    V4,
    V6,
}

impl IpFamily {
    pub fn flag(&self) -> &'static str {
        match self {
            IpFamily::V4 => "-4",
            IpFamily::V6 => "-6",
        }
    }

    pub fn primary_effective_subnet(&self) -> &'static str {
        match self {
            IpFamily::V4 => "0.0.0.0/1",
            IpFamily::V6 => "::/1",
        }
    }

    pub fn split_subnets(&self) -> [&'static str; 2] {
        match self {
            IpFamily::V4 => ["0.0.0.0/1", "128.0.0.0/1"],
            IpFamily::V6 => ["::/1", "8000::/1"],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteEntry {
    pub dst: Option<String>,
    pub gateway: Option<String>,
    pub dev: Option<String>,
    pub protocol: Option<String>,
    pub metric: Option<u32>,
    pub prefsrc: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveRoute {
    pub dev: String,
    pub gateway: Option<String>,
    pub metric: u32,
}

/// Executes `ip <family> -j route show default dev <interface>` and returns the first default route.
pub fn get_default_route_for_dev(
    interface: &str,
    family: IpFamily,
) -> Result<Option<RouteEntry>> {
    let output = Command::new("ip")
        .args([family.flag(), "-j", "route", "show", "default", "dev", interface])
        .output()
        .with_context(|| {
            format!(
                "failed to execute 'ip {} -j route show default dev {interface}'",
                family.flag()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "ip {} route show default dev {} returned error (code {:?}): {}",
            family.flag(),
            interface,
            output.status.code(),
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(None);
    }

    let routes: Vec<RouteEntry> = serde_json::from_str(&stdout).with_context(|| {
        format!(
            "failed to parse JSON from 'ip {} -j route show default dev {interface}': {stdout}",
            family.flag()
        )
    })?;

    // "If multiple default routings of an interface exist, use the first one."
    Ok(routes.into_iter().next())
}

/// Executes `ip <family> -j route show <primary_subnet>` and returns the current effective routing.
pub fn get_current_effective_route(family: IpFamily) -> Result<Option<EffectiveRoute>> {
    let subnet = family.primary_effective_subnet();
    let output = Command::new("ip")
        .args([family.flag(), "-j", "route", "show", subnet])
        .output()
        .with_context(|| {
            format!(
                "failed to execute 'ip {} -j route show {subnet}'",
                family.flag()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "ip {} route show {} returned error (code {:?}): {}",
            family.flag(),
            subnet,
            output.status.code(),
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(None);
    }

    let routes: Vec<RouteEntry> = serde_json::from_str(&stdout).with_context(|| {
        format!(
            "failed to parse JSON from 'ip {} -j route show {subnet}': {stdout}",
            family.flag()
        )
    })?;

    if let Some(first) = routes.into_iter().next() {
        if let Some(dev) = first.dev {
            return Ok(Some(EffectiveRoute {
                dev,
                gateway: first.gateway,
                metric: first.metric.unwrap_or(0),
            }));
        }
    }

    Ok(None)
}

/// Checks if current routing already matches the desired routing.
pub fn is_effective_route_matched(
    current: Option<&EffectiveRoute>,
    target_dev: &str,
    target_gateway: Option<&str>,
    target_metric: u32,
) -> bool {
    if let Some(cur) = current {
        let dev_matches = cur.dev == target_dev;
        let gw_matches = cur.gateway.as_deref() == target_gateway;
        let metric_matches = cur.metric == target_metric;
        dev_matches && gw_matches && metric_matches
    } else {
        false
    }
}

/// Updates system effective routing via `ip <family> route replace`.
pub fn replace_effective_routes(
    target_dev: &str,
    gateway: Option<&str>,
    metric: u32,
    family: IpFamily,
    dry_run: bool,
) -> Result<()> {
    let subnets = family.split_subnets();

    for subnet in subnets {
        let mut cmd = Command::new("ip");
        cmd.args([family.flag(), "route", "replace", subnet, "dev", target_dev]);

        if let Some(gw) = gateway {
            cmd.args(["via", gw]);
        }

        if metric > 0 {
            cmd.args(["metric", &metric.to_string()]);
        }

        let cmd_str = format!("{:?}", cmd);

        if dry_run {
            info!("[DRY RUN] Would execute: {}", cmd_str);
        } else {
            debug!("Executing: {}", cmd_str);
            let output = cmd
                .output()
                .with_context(|| format!("failed to execute route replace command: {cmd_str}"))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(
                    cmd = %cmd_str,
                    exit_code = ?output.status.code(),
                    stderr = %stderr.trim(),
                    "ip route replace command failed"
                );
                anyhow::bail!("ip route replace failed for {subnet}: {}", stderr.trim());
            }
        }
    }

    Ok(())
}

/// Clears / flushes conntrack entries by running `conntrack -F`.
pub fn flush_conntrack(dry_run: bool) -> Result<()> {
    if dry_run {
        info!("[DRY RUN] Would execute: conntrack -F");
        return Ok(());
    }

    debug!("Executing: conntrack -F");
    match Command::new("conntrack").arg("-F").output() {
        Ok(output) => {
            if output.status.success() {
                info!("Successfully flushed conntrack table (conntrack -F)");
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(
                    exit_code = ?output.status.code(),
                    stderr = %stderr.trim(),
                    "conntrack -F returned non-zero status"
                );
            }
            Ok(())
        }
        Err(e) => {
            warn!(
                error = %e,
                "Failed to execute 'conntrack -F' (conntrack-tools may not be installed)"
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_default_route_json() {
        let json_data = r#"[
            {"dst":"default","gateway":"172.24.1.254","protocol":"static","metric":100,"flags":[]},
            {"dst":"default","gateway":"172.24.1.1","protocol":"static","metric":200,"flags":[]}
        ]"#;

        let routes: Vec<RouteEntry> = serde_json::from_str(json_data).unwrap();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].gateway.as_deref(), Some("172.24.1.254"));
        assert_eq!(routes[0].metric, Some(100));
        assert_eq!(routes[0].dst.as_deref(), Some("default"));
    }

    #[test]
    fn test_parse_prompt_example_routes() {
        let json1 = r#"[{"dst":"default","gateway":"172.24.1.254","protocol":"static","metric":100,"flags":[]}]"#;
        let routes1: Vec<RouteEntry> = serde_json::from_str(json1).unwrap();
        assert_eq!(routes1[0].gateway.as_deref(), Some("172.24.1.254"));
        assert_eq!(routes1[0].metric, Some(100));

        let json2 = r#"[{"dst":"default","gateway":"10.40.151.254","protocol":"static","prefsrc":"10.40.150.171","metric":200,"flags":[]}]"#;
        let routes2: Vec<RouteEntry> = serde_json::from_str(json2).unwrap();
        assert_eq!(routes2[0].gateway.as_deref(), Some("10.40.151.254"));
        assert_eq!(routes2[0].prefsrc.as_deref(), Some("10.40.150.171"));
        assert_eq!(routes2[0].metric, Some(200));
    }

    #[test]
    fn test_parse_effective_route_json() {
        let json = r#"[{"dst":"0.0.0.0/1","gateway":"172.24.1.254","dev":"wan","flags":[]}]"#;
        let routes: Vec<RouteEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(routes[0].dev.as_deref(), Some("wan"));
        assert_eq!(routes[0].gateway.as_deref(), Some("172.24.1.254"));
        assert_eq!(routes[0].metric, None);
    }

    #[test]
    fn test_parse_effective_v6_route_json() {
        let json = r#"[{"dst":"::/1","gateway":"fe80::1","dev":"wan","metric":100,"flags":[]}]"#;
        let routes: Vec<RouteEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(routes[0].dev.as_deref(), Some("wan"));
        assert_eq!(routes[0].gateway.as_deref(), Some("fe80::1"));
        assert_eq!(routes[0].metric, Some(100));
    }

    #[test]
    fn test_is_effective_route_matched() {
        let current = EffectiveRoute {
            dev: "wan".to_string(),
            gateway: Some("172.24.1.254".to_string()),
            metric: 0,
        };

        assert!(is_effective_route_matched(
            Some(&current),
            "wan",
            Some("172.24.1.254"),
            0
        ));

        assert!(!is_effective_route_matched(
            Some(&current),
            "phy1-sta0",
            Some("10.40.151.254"),
            0
        ));

        assert!(!is_effective_route_matched(
            Some(&current),
            "wan",
            Some("172.24.1.254"),
            10
        ));

        assert!(!is_effective_route_matched(None, "wan", Some("172.24.1.254"), 0));
    }
}
