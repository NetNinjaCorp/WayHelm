use std::process::Command;

use anyhow::{anyhow, Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Firewall {
    Ufw,
    Firewalld,
    Nftables,
}

impl Firewall {
    pub fn label(self) -> &'static str {
        match self {
            Firewall::Ufw => "ufw",
            Firewall::Firewalld => "firewalld",
            Firewall::Nftables => "nftables",
        }
    }
}

pub fn detect_active() -> Option<Firewall> {
    if is_active("ufw") {
        return Some(Firewall::Ufw);
    }
    if is_active("firewalld") {
        return Some(Firewall::Firewalld);
    }
    if is_active("nftables") {
        return Some(Firewall::Nftables);
    }
    None
}

fn is_active(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", unit])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
pub enum Scope {
    Lan(String),
    Any,
}

pub fn open_port(fw: Firewall, port: u16, scope: &Scope) -> Result<()> {
    // Belt-and-braces CIDR sanity check at the privileged-execution boundary.
    // The current detect_primary_lan_cidr() can only produce a well-formed
    // address, but this guards against any future caller or refactor that
    // might let user-controlled text reach a `pkexec sh -c …{cidr}…` path.
    if let Scope::Lan(cidr) = scope {
        if !is_valid_cidr(cidr) {
            return Err(anyhow!("refusing to use malformed CIDR {cidr:?}"));
        }
    }
    match fw {
        Firewall::Ufw => open_ufw(port, scope),
        Firewall::Firewalld => open_firewalld(port, scope),
        Firewall::Nftables => Err(anyhow!(
            "Wayhelm doesn't automate raw nftables. Add a rule for TCP port {port} manually."
        )),
    }
}

fn is_valid_cidr(cidr: &str) -> bool {
    let Some((ip, prefix)) = cidr.split_once('/') else {
        return false;
    };
    let v4 = ip.parse::<std::net::Ipv4Addr>().is_ok();
    let v6 = ip.parse::<std::net::Ipv6Addr>().is_ok();
    if !v4 && !v6 {
        return false;
    }
    let Ok(p) = prefix.parse::<u32>() else {
        return false;
    };
    if v4 {
        p <= 32
    } else {
        p <= 128
    }
}

fn open_ufw(port: u16, scope: &Scope) -> Result<()> {
    let port_s = port.to_string();
    match scope {
        Scope::Lan(cidr) => pkexec(&[
            "ufw", "allow", "from", cidr.as_str(), "to", "any", "port", &port_s, "proto", "tcp",
        ]),
        Scope::Any => {
            let rule = format!("{port}/tcp");
            pkexec(&["ufw", "allow", &rule])
        }
    }
}

fn open_firewalld(port: u16, scope: &Scope) -> Result<()> {
    // firewalld changes a single port via two separate commands (add + reload).
    // pkexec only runs one binary per invocation, so we wrap the pair in `sh -c`.
    let script = match scope {
        Scope::Lan(cidr) => format!(
            "firewall-cmd --permanent \
             --add-rich-rule='rule family=\"ipv4\" source address=\"{cidr}\" \
             port port=\"{port}\" protocol=\"tcp\" accept' && firewall-cmd --reload",
        ),
        Scope::Any => format!(
            "firewall-cmd --permanent --add-port={port}/tcp && firewall-cmd --reload",
        ),
    };
    pkexec(&["sh", "-c", &script])
}

fn pkexec(args: &[&str]) -> Result<()> {
    let out = Command::new("pkexec")
        .args(args)
        .output()
        .context("spawning pkexec")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let code = out.status.code().unwrap_or(-1);
        // pkexec exits 126 if the user dismissed the auth prompt, 127 if not authorized.
        let hint = match code {
            126 => " (authentication cancelled)",
            127 => " (not authorized)",
            _ => "",
        };
        return Err(anyhow!(
            "pkexec exit {code}{hint}: {}",
            if stderr.is_empty() {
                "(no stderr)".to_string()
            } else {
                stderr
            }
        ));
    }
    Ok(())
}

/// Find the CIDR of the interface used for the default IPv4 route, expressed
/// as a network address (e.g. "10.2.0.0/24" rather than the host's own IP).
pub fn detect_primary_lan_cidr() -> Option<String> {
    let route = Command::new("ip")
        .args(["-json", "route", "show", "default"])
        .output()
        .ok()?;
    if !route.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&route.stdout).ok()?;
    let dev = v.as_array()?.first()?.get("dev")?.as_str()?;
    let addrs = Command::new("ip")
        .args(["-json", "-4", "addr", "show", "dev", dev])
        .output()
        .ok()?;
    if !addrs.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&addrs.stdout).ok()?;
    let addr_info = v.as_array()?.first()?.get("addr_info")?.as_array()?;
    for a in addr_info {
        let local = a.get("local")?.as_str()?;
        let prefixlen = a.get("prefixlen")?.as_u64()? as u32;
        let ip: std::net::Ipv4Addr = local.parse().ok()?;
        let host = u32::from_be_bytes(ip.octets());
        let mask = if prefixlen == 0 {
            0
        } else {
            (!0u32).checked_shl(32 - prefixlen).unwrap_or(0)
        };
        let netip = std::net::Ipv4Addr::from((host & mask).to_be_bytes());
        return Some(format!("{netip}/{prefixlen}"));
    }
    None
}
