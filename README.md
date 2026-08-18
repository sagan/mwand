# mwand

A simplified & lightweight alternative of OpenWrt `mwan3` written in Rust.

`mwand` runs as a single Linux CLI daemon that monitors multiple WAN interfaces and performs automated failover when the active WAN connection goes down.

## Features

- **Dual-Stack Support by Default**: Handles both IPv4 (`0.0.0.0/1` + `128.0.0.0/1`) and IPv6 (`::/1` + `8000::/1`) routing tables simultaneously.
- **Protocol Filtering**: Optional `-4` (`--ipv4`) and `-6` (`--ipv6`) mutually exclusive flags to operate in IPv4-only or IPv6-only mode.
- **Pure Rust ICMP / ICMPv6 Pinging**: Implements socket-level ICMP Echo Request / Reply with `SO_BINDTODEVICE` (`ping -I <dev> <ip>`) without calling any external `ping` binary.
- **Conntrack Flushing**: Automatically flushes the conntrack table (`conntrack -F`) on failover route changes to drop stale connections (with `--no-conntrack-flush` flag to disable).
- **Automatic Gateway Discovery**: Automatically queries interface gateway information using `ip <family> -j route show default dev <interface>`.
- **Minimal Routing Changes**: Checks current effective routing using `ip <family> -j route show <subnet>` and only updates routing rules when the desired active interface or gateway changes.
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

- `<INTERFACES>...`: WAN interface names in order of preference (e.g. `wan phy1-sta0`). The first listed interface has highest priority.

### Protocol Options

- `-4`, `--ipv4`: Only manage IPv4 routing and health checks (mutually exclusive with `-6`).
- `-6`, `--ipv6`: Only manage IPv6 routing and health checks (mutually exclusive with `-4`).
- *(Default: Both IPv4 and IPv6 routing are managed)*

### General Options

- `--ip <IP4>`, `--ip4 <IP4>`: Ping test target IPv4 address (default: `223.5.5.5`).
- `--ip6 <IP6>`: Ping test target IPv6 address (default: `2400:3200::1`).
- `--metric <METRIC>`: Optional routing metric for split subnet routes (default: `0`).
- `--interval <SECONDS>`: Interval between health check cycles in seconds (default: `3`).
- `--timeout <MS>`: Ping timeout in milliseconds (default: `1000`).
- `--count <COUNT>`: Number of ping probes per health check cycle (default: `1`).
- `--no-conntrack-flush`, `--no-conntrack`: Disable running `conntrack -F` when routing updates.
- `--dry-run`: Simulate routing table changes without running `ip route replace` or `conntrack -F`.
- `--once`: Execute a single health check cycle and exit.

## Examples

### Dual-Stack Failover (Default)

Monitors and updates both IPv4 and IPv6 routes, and runs `conntrack -F` on failover:

```bash
mwand wan phy1-sta0
```

### IPv4-Only Mode

```bash
mwand -4 wan phy1-sta0 --ip 1.1.1.1
```

### IPv6-Only Mode

```bash
mwand -6 wan phy1-sta0 --ip6 2606:4700:4700::1111
```

### Disable Conntrack Flushing

```bash
mwand wan phy1-sta0 --no-conntrack-flush
```

### Dry Run Test

```bash
mwand wan phy1-sta0 --dry-run --once
```
