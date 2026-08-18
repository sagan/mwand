use clap::Parser;
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Parser, Debug, Clone)]
#[command(
    name = "mwand",
    version,
    about = "Lightweight Multi-WAN failover daemon for Linux (IPv4 & IPv6)",
    long_about = "A simplified & lightweight alternative of OpenWrt mwan3 in Rust.\nMonitors WAN interfaces via ICMP/ICMPv6 pings and updates 0.0.0.0/1 + 128.0.0.0/1 (and ::/1 + 8000::/1) routing tables for failover."
)]
pub struct Cli {
    /// WAN interfaces in order of preference/priority (e.g. wan phy1-sta0)
    #[arg(required = true, num_args = 1..)]
    pub interfaces: Vec<String>,

    /// Only manage IPv4 routing and health checks (mutually exclusive with -6)
    #[arg(short = '4', long = "ipv4", conflicts_with = "ipv6")]
    pub ipv4: bool,

    /// Only manage IPv6 routing and health checks (mutually exclusive with -4)
    #[arg(short = '6', long = "ipv6", conflicts_with = "ipv4")]
    pub ipv6: bool,

    /// Ping test target IPv4 address
    #[arg(long = "ip", alias = "ip4", default_value = "223.5.5.5")]
    pub ip4: Ipv4Addr,

    /// Ping test target IPv6 address
    #[arg(long = "ip6", default_value = "2400:3200::1")]
    pub ip6: Ipv6Addr,

    /// Optional routing metric when running 'ip route replace' (default is 0)
    #[arg(long, default_value_t = 0)]
    pub metric: u32,

    /// Interval between ping health checks in seconds
    #[arg(long, default_value_t = 15)]
    pub interval: u64,

    /// Ping timeout per probe in milliseconds
    #[arg(long, default_value_t = 1000)]
    pub timeout: u64,

    /// Number of ping probes per health check cycle
    #[arg(long, default_value_t = 3)]
    pub count: u32,

    /// Disable flushing conntrack table ('conntrack -F') when routing updates
    #[arg(long = "no-conntrack-flush", alias = "no-conntrack")]
    pub no_conntrack_flush: bool,

    /// Dry run mode (simulate routing table changes without executing ip route replace)
    #[arg(long)]
    pub dry_run: bool,

    /// Run only a single health check cycle and exit
    #[arg(long)]
    pub once: bool,
}

impl Cli {
    /// Determines whether IPv4 is enabled. (True by default, unless -6 is specified)
    pub fn is_ipv4_enabled(&self) -> bool {
        if self.ipv6 {
            false
        } else {
            true
        }
    }

    /// Determines whether IPv6 is enabled. (True by default, unless -4 is specified)
    pub fn is_ipv6_enabled(&self) -> bool {
        if self.ipv4 {
            false
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_cli_defaults() {
        let args = Cli::try_parse_from(["mwand", "wan", "phy1-sta0"]).unwrap();
        assert_eq!(args.interfaces, vec!["wan", "phy1-sta0"]);
        assert!(args.is_ipv4_enabled());
        assert!(args.is_ipv6_enabled());
        assert_eq!(args.ip4.to_string(), "223.5.5.5");
        assert_eq!(args.ip6.to_string(), "2400:3200::1");
        assert_eq!(args.metric, 0);
        assert_eq!(args.interval, 15);
        assert_eq!(args.timeout, 1000);
        assert_eq!(args.count, 3);
        assert!(!args.no_conntrack_flush);
        assert!(!args.dry_run);
        assert!(!args.once);
    }

    #[test]
    fn test_cli_ipv4_only() {
        let args = Cli::try_parse_from(["mwand", "-4", "wan"]).unwrap();
        assert!(args.is_ipv4_enabled());
        assert!(!args.is_ipv6_enabled());
    }

    #[test]
    fn test_cli_ipv6_only() {
        let args = Cli::try_parse_from(["mwand", "-6", "wan"]).unwrap();
        assert!(!args.is_ipv4_enabled());
        assert!(args.is_ipv6_enabled());
    }

    #[test]
    fn test_cli_ipv4_ipv6_mutually_exclusive() {
        assert!(Cli::try_parse_from(["mwand", "-4", "-6", "wan"]).is_err());
    }

    #[test]
    fn test_cli_custom_flags() {
        let args = Cli::try_parse_from([
            "mwand",
            "eth0",
            "wlan0",
            "--ip",
            "1.1.1.1",
            "--ip6",
            "2606:4700:4700::1111",
            "--metric",
            "50",
            "--interval",
            "10",
            "--timeout",
            "2000",
            "--count",
            "3",
            "--no-conntrack-flush",
            "--dry-run",
            "--once",
        ])
        .unwrap();

        assert_eq!(args.interfaces, vec!["eth0", "wlan0"]);
        assert_eq!(args.ip4.to_string(), "1.1.1.1");
        assert_eq!(args.ip6.to_string(), "2606:4700:4700::1111");
        assert_eq!(args.metric, 50);
        assert_eq!(args.interval, 10);
        assert_eq!(args.timeout, 2000);
        assert_eq!(args.count, 3);
        assert!(args.no_conntrack_flush);
        assert!(args.dry_run);
        assert!(args.once);
    }

    #[test]
    fn test_cli_missing_interfaces() {
        assert!(Cli::try_parse_from(["mwand"]).is_err());
    }
}
