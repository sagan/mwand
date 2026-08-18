use anyhow::{Context, Result};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

const ICMP_ECHO_REQUEST: u8 = 8;
const ICMP_ECHO_REPLY: u8 = 0;

const ICMP6_ECHO_REQUEST: u8 = 128;
const ICMP6_ECHO_REPLY: u8 = 129;

static SEQ_COUNTER: AtomicU16 = AtomicU16::new(1);
static SEQ6_COUNTER: AtomicU16 = AtomicU16::new(1);

/// Computes standard RFC 1071 16-bit Internet Checksum (for IPv4 ICMP).
pub fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut chunks = data.chunks_exact(2);
    for chunk in &mut chunks {
        let word = u16::from_be_bytes([chunk[0], chunk[1]]);
        sum = sum.wrapping_add(word as u32);
    }
    let remainder = chunks.remainder();
    if !remainder.is_empty() {
        let word = u16::from_be_bytes([remainder[0], 0]);
        sum = sum.wrapping_add(word as u32);
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// Pings an IPv4 target bound strictly to a network interface device via SO_BINDTODEVICE.
pub fn ping_v4_device(
    interface: &str,
    target_ip: Ipv4Addr,
    timeout: Duration,
) -> Result<Duration> {
    let (socket, is_raw) = match Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4)) {
        Ok(s) => (s, true),
        Err(raw_err) => {
            let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).map_err(|dgram_err| {
                anyhow::anyhow!(
                    "failed to create ICMP raw socket ({}) or dgram socket ({})",
                    raw_err,
                    dgram_err
                )
            })?;
            (s, false)
        }
    };

    socket
        .bind_device(Some(interface.as_bytes()))
        .with_context(|| format!("failed to bind IPv4 socket to device '{interface}'"))?;

    socket.set_read_timeout(Some(timeout))?;
    socket.set_write_timeout(Some(timeout))?;

    let pid = (std::process::id() & 0xFFFF) as u16;
    let seq = SEQ_COUNTER.fetch_add(1, Ordering::Relaxed);

    let payload = b"mwand-probe-health-check-v4";
    let mut packet = vec![0u8; 8 + payload.len()];
    packet[0] = ICMP_ECHO_REQUEST;
    packet[1] = 0; // code
    packet[2] = 0; // checksum msb
    packet[3] = 0; // checksum lsb
    packet[4..6].copy_from_slice(&pid.to_be_bytes());
    packet[6..8].copy_from_slice(&seq.to_be_bytes());
    packet[8..].copy_from_slice(payload);

    let csum = internet_checksum(&packet);
    packet[2..4].copy_from_slice(&csum.to_be_bytes());

    let dest_addr = SocketAddr::V4(SocketAddrV4::new(target_ip, 0));
    let sock_addr = socket2::SockAddr::from(dest_addr);

    let start = Instant::now();
    socket
        .send_to(&packet, &sock_addr)
        .with_context(|| format!("failed to send ICMP Echo Request on '{interface}' to {target_ip}"))?;

    let mut buf = [std::mem::MaybeUninit::<u8>::uninit(); 1500];

    loop {
        if start.elapsed() >= timeout {
            anyhow::bail!("ping timeout after {:?}", timeout);
        }

        let (bytes_read, _peer) = match socket.recv_from(&mut buf) {
            Ok(res) => res,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock {
                    anyhow::bail!("ping timed out on '{interface}' to {target_ip}");
                }
                return Err(e).with_context(|| format!("recv_from failed on '{interface}'"));
            }
        };

        let rtt = start.elapsed();

        let slice: &[u8] = unsafe {
            std::slice::from_raw_parts(buf.as_ptr() as *const u8, bytes_read)
        };

        if is_raw {
            if slice.len() < 20 + 8 {
                continue;
            }
            let ihl = (slice[0] & 0x0f) as usize * 4;
            if slice.len() < ihl + 8 {
                continue;
            }
            let icmp_slice = &slice[ihl..];
            let icmp_type = icmp_slice[0];
            let icmp_code = icmp_slice[1];
            let reply_id = u16::from_be_bytes([icmp_slice[4], icmp_slice[5]]);
            let reply_seq = u16::from_be_bytes([icmp_slice[6], icmp_slice[7]]);

            if icmp_type == ICMP_ECHO_REPLY && icmp_code == 0 && reply_id == pid && reply_seq == seq {
                debug!(interface = %interface, ?rtt, "ICMP Echo Reply received");
                return Ok(rtt);
            }
        } else {
            if slice.len() < 8 {
                continue;
            }
            let icmp_type = slice[0];
            let icmp_code = slice[1];
            let reply_seq = u16::from_be_bytes([slice[6], slice[7]]);

            if icmp_type == ICMP_ECHO_REPLY && icmp_code == 0 && reply_seq == seq {
                debug!(interface = %interface, ?rtt, "ICMP Echo Reply received (dgram)");
                return Ok(rtt);
            }
        }
    }
}

/// Pings an IPv6 target bound strictly to a network interface device via SO_BINDTODEVICE.
pub fn ping_v6_device(
    interface: &str,
    target_ip: Ipv6Addr,
    timeout: Duration,
) -> Result<Duration> {
    let (socket, _is_raw) = match Socket::new(Domain::IPV6, Type::RAW, Some(Protocol::ICMPV6)) {
        Ok(s) => (s, true),
        Err(raw_err) => {
            let s = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::ICMPV6)).map_err(|dgram_err| {
                anyhow::anyhow!(
                    "failed to create ICMPv6 raw socket ({}) or dgram socket ({})",
                    raw_err,
                    dgram_err
                )
            })?;
            (s, false)
        }
    };

    socket
        .bind_device(Some(interface.as_bytes()))
        .with_context(|| format!("failed to bind IPv6 socket to device '{interface}'"))?;

    socket.set_read_timeout(Some(timeout))?;
    socket.set_write_timeout(Some(timeout))?;

    let pid = (std::process::id() & 0xFFFF) as u16;
    let seq = SEQ6_COUNTER.fetch_add(1, Ordering::Relaxed);

    let payload = b"mwand-probe-health-check-v6";
    let mut packet = vec![0u8; 8 + payload.len()];
    packet[0] = ICMP6_ECHO_REQUEST;
    packet[1] = 0; // code
    packet[2] = 0; // checksum msb (handled by kernel for raw ICMPv6)
    packet[3] = 0; // checksum lsb (handled by kernel for raw ICMPv6)
    packet[4..6].copy_from_slice(&pid.to_be_bytes());
    packet[6..8].copy_from_slice(&seq.to_be_bytes());
    packet[8..].copy_from_slice(payload);

    let dest_addr = SocketAddr::V6(SocketAddrV6::new(target_ip, 0, 0, 0));
    let sock_addr = socket2::SockAddr::from(dest_addr);

    let start = Instant::now();
    socket
        .send_to(&packet, &sock_addr)
        .with_context(|| format!("failed to send ICMPv6 Echo Request on '{interface}' to {target_ip}"))?;

    let mut buf = [std::mem::MaybeUninit::<u8>::uninit(); 1500];

    loop {
        if start.elapsed() >= timeout {
            anyhow::bail!("ping6 timeout after {:?}", timeout);
        }

        let (bytes_read, _peer) = match socket.recv_from(&mut buf) {
            Ok(res) => res,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::TimedOut || e.kind() == std::io::ErrorKind::WouldBlock {
                    anyhow::bail!("ping6 timed out on '{interface}' to {target_ip}");
                }
                return Err(e).with_context(|| format!("recv_from failed on '{interface}'"));
            }
        };

        let rtt = start.elapsed();

        let slice: &[u8] = unsafe {
            std::slice::from_raw_parts(buf.as_ptr() as *const u8, bytes_read)
        };

        // On Linux, ICMPv6 raw sockets return ICMPv6 packet directly
        if slice.len() >= 8 {
            let icmp_type = slice[0];
            let icmp_code = slice[1];
            let reply_id = u16::from_be_bytes([slice[4], slice[5]]);
            let reply_seq = u16::from_be_bytes([slice[6], slice[7]]);

            if icmp_type == ICMP6_ECHO_REPLY && icmp_code == 0 && reply_id == pid && reply_seq == seq {
                debug!(interface = %interface, ?rtt, "ICMPv6 Echo Reply received");
                return Ok(rtt);
            }
        }
    }
}

/// Generic interface ping dispatcher for either IPv4 or IPv6.
pub fn ping_ip_device(
    interface: &str,
    target_ip: IpAddr,
    timeout: Duration,
) -> Result<Duration> {
    match target_ip {
        IpAddr::V4(v4) => ping_v4_device(interface, v4, timeout),
        IpAddr::V6(v6) => ping_v6_device(interface, v6, timeout),
    }
}

/// Tests interface health for a given target IP address.
pub fn check_interface_health(
    interface: &str,
    target_ip: IpAddr,
    timeout: Duration,
    count: u32,
) -> bool {
    let probes = count.max(1);
    for probe_idx in 1..=probes {
        match ping_ip_device(interface, target_ip, timeout) {
            Ok(rtt) => {
                debug!(
                    interface = %interface,
                    target = %target_ip,
                    probe = probe_idx,
                    ?rtt,
                    "Health check ping succeeded"
                );
                return true;
            }
            Err(e) => {
                debug!(
                    interface = %interface,
                    target = %target_ip,
                    probe = probe_idx,
                    error = %e,
                    "Health check probe failed"
                );
            }
        }
    }
    warn!(
        interface = %interface,
        target = %target_ip,
        probes = probes,
        "Health check ping failed on interface"
    );
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checksum_calculation() {
        let data = [
            0x08, 0x00, 0x00, 0x00, // Type 8, Code 0, Checksum 0
            0x12, 0x34, 0x00, 0x01, // ID, Seq
            b'T', b'E', b'S', b'T', // Payload
        ];
        let csum = internet_checksum(&data);
        assert_ne!(csum, 0);

        let mut verified = data.to_vec();
        verified[2..4].copy_from_slice(&csum.to_be_bytes());
        assert_eq!(internet_checksum(&verified), 0);
    }
}
