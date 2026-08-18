use anyhow::Result;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::cli::Cli;
use crate::pinger;
use crate::router::{self, IpFamily};

pub fn run_daemon(args: Cli) -> Result<()> {
    info!(
        interfaces = ?args.interfaces,
        ipv4_enabled = args.is_ipv4_enabled(),
        ipv6_enabled = args.is_ipv6_enabled(),
        target_ip4 = %args.ip4,
        target_ip6 = %args.ip6,
        metric = args.metric,
        interval_secs = args.interval,
        timeout_ms = args.timeout,
        no_conntrack_flush = args.no_conntrack_flush,
        dry_run = args.dry_run,
        "Starting mwand daemon"
    );

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc_handler(r)?;

    let timeout = Duration::from_millis(args.timeout);
    let interval = Duration::from_secs(args.interval);

    let mut last_reported_v4_active: Option<String> = None;
    let mut last_reported_v6_active: Option<String> = None;

    while running.load(Ordering::SeqCst) {
        let mut routes_updated = false;

        // Process IPv4 failover if enabled
        if args.is_ipv4_enabled() {
            let updated = process_family(
                &args,
                IpFamily::V4,
                IpAddr::V4(args.ip4),
                timeout,
                &mut last_reported_v4_active,
            );
            if updated {
                routes_updated = true;
            }
        }

        // Process IPv6 failover if enabled
        if args.is_ipv6_enabled() {
            let updated = process_family(
                &args,
                IpFamily::V6,
                IpAddr::V6(args.ip6),
                timeout,
                &mut last_reported_v6_active,
            );
            if updated {
                routes_updated = true;
            }
        }

        // Flush conntrack if any effective route was modified and not disabled
        if routes_updated {
            if !args.no_conntrack_flush {
                if let Err(e) = router::flush_conntrack(args.dry_run) {
                    warn!(error = %e, "Failed to flush conntrack table");
                }
            } else {
                debug!("Conntrack flush skipped as requested by --no-conntrack-flush");
            }
        }

        if args.once {
            break;
        }

        // Sleep in small increments to be responsive to termination signals
        let sleep_step = Duration::from_millis(200);
        let mut elapsed = Duration::from_millis(0);
        while elapsed < interval && running.load(Ordering::SeqCst) {
            thread::sleep(sleep_step);
            elapsed += sleep_step;
        }
    }

    info!("mwand daemon stopped cleanly.");
    Ok(())
}

fn process_family(
    args: &Cli,
    family: IpFamily,
    target_ip: IpAddr,
    timeout: Duration,
    last_reported: &mut Option<String>,
) -> bool {
    let family_name = match family {
        IpFamily::V4 => "IPv4",
        IpFamily::V6 => "IPv6",
    };

    let mut health_map = HashMap::new();

    for iface in &args.interfaces {
        let is_healthy = pinger::check_interface_health(iface, target_ip, timeout, args.count);
        health_map.insert(iface.clone(), is_healthy);
    }

    // Find highest priority healthy interface
    let best_healthy = args
        .interfaces
        .iter()
        .find(|&iface| health_map.get(iface).copied().unwrap_or(false));

    match best_healthy {
        None => {
            warn!(
                family = family_name,
                target_ip = %target_ip,
                "All WAN interfaces failed health check. Keeping status quo (no route changes)."
            );
            false
        }
        Some(target_dev) => {
            // Get current effective routing
            let current_effective = match router::get_current_effective_route(family) {
                Ok(eff) => eff,
                Err(e) => {
                    error!(
                        family = family_name,
                        error = %e,
                        "Failed to query current effective routing"
                    );
                    None
                }
            };

            // Get default route info for the target healthy interface
            match router::get_default_route_for_dev(target_dev, family) {
                Ok(Some(dev_route)) => {
                    let target_gw = dev_route.gateway.as_deref();

                    if router::is_effective_route_matched(
                        current_effective.as_ref(),
                        target_dev,
                        target_gw,
                        args.metric,
                    ) {
                        if last_reported.as_deref() != Some(target_dev.as_str()) {
                            info!(
                                family = family_name,
                                interface = %target_dev,
                                gateway = ?target_gw,
                                metric = args.metric,
                                "Active WAN interface is healthy"
                            );
                            *last_reported = Some(target_dev.clone());
                        } else {
                            debug!(
                                family = family_name,
                                interface = %target_dev,
                                "Active WAN route unchanged and healthy"
                            );
                        }
                        false
                    } else {
                        let old_desc = current_effective
                            .as_ref()
                            .map(|c| format!("{} (via {:?})", c.dev, c.gateway))
                            .unwrap_or_else(|| "none".to_string());

                        info!(
                            family = family_name,
                            previous = %old_desc,
                            target_dev = %target_dev,
                            gateway = ?target_gw,
                            metric = args.metric,
                            "Failover: updating effective routing..."
                        );

                        if let Err(e) = router::replace_effective_routes(
                            target_dev,
                            target_gw,
                            args.metric,
                            family,
                            args.dry_run,
                        ) {
                            error!(
                                family = family_name,
                                error = %e,
                                target_dev = %target_dev,
                                "Failed to update effective routes"
                            );
                            false
                        } else {
                            info!(
                                family = family_name,
                                target_dev = %target_dev,
                                gateway = ?target_gw,
                                metric = args.metric,
                                "Successfully updated effective routing to active WAN"
                            );
                            *last_reported = Some(target_dev.clone());
                            true
                        }
                    }
                }
                Ok(None) => {
                    warn!(
                        family = family_name,
                        interface = %target_dev,
                        "Interface is healthy but has no default route in routing table"
                    );
                    false
                }
                Err(e) => {
                    error!(
                        family = family_name,
                        interface = %target_dev,
                        error = %e,
                        "Failed to get default route for interface"
                    );
                    false
                }
            }
        }
    }
}

fn ctrlc_handler(running: Arc<AtomicBool>) -> Result<()> {
    unsafe {
        libc::signal(libc::SIGINT, sig_handler as *const () as usize);
        libc::signal(libc::SIGTERM, sig_handler as *const () as usize);
    }
    GLOBAL_RUNNING.store(true, Ordering::SeqCst);
    RUNNING_REF.lock().unwrap().replace(running);
    Ok(())
}

static GLOBAL_RUNNING: AtomicBool = AtomicBool::new(true);
static RUNNING_REF: std::sync::Mutex<Option<Arc<AtomicBool>>> = std::sync::Mutex::new(None);

extern "C" fn sig_handler(_sig: libc::c_int) {
    GLOBAL_RUNNING.store(false, Ordering::SeqCst);
    if let Ok(guard) = RUNNING_REF.lock() {
        if let Some(r) = guard.as_ref() {
            r.store(false, Ordering::SeqCst);
        }
    }
}
