# mwand

A simplified & lightweight alternative of OpenWrt `mwan3` written in Rust.

`mwand` runs as a single Linux CLI daemon that monitors multiple WAN interfaces and performs automated failover when the active WAN connection goes down.

## Features

- **Dual-Stack Support by Default**: Handles both IPv4 (`0.0.0.0/1` + `128.0.0.0/1` or `0.0.0.0/0`) and IPv6 (`::/1` + `8000::/1` or `::/0`) routing tables simultaneously.
- **Explicit or Auto-Discovered Route Parameters**: Supports custom comma-separated routing parameters (e.g. `"wan,via 172.24.1.254"` or `"wan,,via fe80::1"`), bypassing default gateway lookups when provided.
- **Custom Routing Table Support**: Configure target routing table using `--table <number>` (defaults to `254` / main table).
- **Default Route Mode (`-0`)**: Direct replacement of `0.0.0.0/0` (and `::/0`) routes instead of split subnets (`0.0.0.0/1` + `128.0.0.0/1`).
- **Protocol Filtering**: Optional `-4` (`--ipv4`) and `-6` (`--ipv6`) mutually exclusive flags to operate in IPv4-only or IPv6-only mode.
- **Pure Rust ICMP / ICMPv6 Pinging**: Implements socket-level ICMP Echo Request / Reply with `SO_BINDTODEVICE` (`ping -I <dev> <ip>`) without calling any external `ping` binary.
- **Conntrack Flushing**: Automatically flushes the conntrack table (`conntrack -F`) on failover route changes to drop stale connections (with `--no-conntrack-flush` flag to disable).
- **Minimal Routing Changes**: Checks current effective routing using `ip <family> -j route show <subnet> table <table>` and only updates routing rules when the desired active interface or gateway changes.
- **Status Quo Protection**: If all WAN interfaces fail the health check, the current routing status quo is maintained.

## Installation / Build

```bash
cargo build --release
```

The resulting binary will be located at `target/release/mwand`.

## Usage

```bash
mwand [OPTIONS] <INTERFACES>...
```

### Positional Arguments

- `<INTERFACES>...`: Comma-separated interface and routing parameters in order of preference (e.g. `"wan,via 172.24.1.254"` `"phy1-sta0"` or `"wan,,via fe80::1"`).
  - Format: `<interface>[,<ipv4_route_params>[,<ipv6_route_params>]]`
  - If custom route parameters are provided, automatic `ip route show default dev <interface>` lookup is skipped for that family.

### Protocol Options

- `-4`, `--ipv4`: Only manage IPv4 routing and health checks (mutually exclusive with `-6`).
- `-6`, `--ipv6`: Only manage IPv6 routing and health checks (mutually exclusive with `-4`).
- *(Default: Both IPv4 and IPv6 routing are managed)*

### Routing & Health Check Options

- `-0`, `--default-route`: Update default routing `0.0.0.0/0` (and `::/0`) directly instead of using `0.0.0.0/1` + `128.0.0.0/1` (and `::/1` + `8000::/1`).
- `--table <NUMBER>`: Routing table number for route lookup and replacement (default: `254` / main table).
- `--ip <IP4>`, `--ip4 <IP4>`: Ping test target IPv4 address (default: `223.5.5.5`).
- `--ip6 <IP6>`: Ping test target IPv6 address (default: `2400:3200::1`).
- `--metric <METRIC>`: Optional routing metric for routes (default: `0`).
- `--interval <SECONDS>`: Interval between health check cycles in seconds (default: `15`).
- `--timeout <MS>`: Ping timeout in milliseconds (default: `1000`).
- `--count <COUNT>`: Number of ping probes per health check cycle (default: `3`).
- `--no-conntrack-flush`, `--no-conntrack`: Disable running `conntrack -F` when routing updates.
- `--dry-run`: Simulate routing table changes without running `ip route replace` or `conntrack -F`.
- `--once`: Execute a single health check cycle and exit.

## Examples

### Custom Gateway Parameters

```bash
mwand "wan,via 172.24.1.254" "phy1-sta0"
```

### Custom IPv6 Gateway Parameter

```bash
mwand -6 "wan,,via fe80::1" "phy1-sta0"
```

### Direct 0.0.0.0/0 Default Route and Custom Table

```bash
mwand -0 --table 100 "wan,via 172.24.1.254" "phy1-sta0"
```

### Dual-Stack Failover (Default)

```bash
mwand "wan,via 172.24.1.254,via fe80::1" "phy1-sta0"
```
