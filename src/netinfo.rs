use std::net::IpAddr;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Iface {
    pub name: String,
    pub addr: IpAddr,
}

/// Enumerate non-loopback IP addresses by shelling out to `ip -json addr`.
/// Avoids adding a heavyweight netlink dependency for what is, on Linux,
/// a one-line query.
pub fn local_addresses() -> Vec<Iface> {
    let Ok(out) = Command::new("ip").args(["-json", "addr", "show"]).output() else {
        return vec![];
    };
    if !out.status.success() {
        return vec![];
    }
    let Ok(v): Result<serde_json::Value, _> = serde_json::from_slice(&out.stdout) else {
        return vec![];
    };
    let Some(ifs) = v.as_array() else {
        return vec![];
    };

    let mut result = Vec::new();
    for ifc in ifs {
        let name = ifc
            .get("ifname")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if name == "lo" {
            continue;
        }
        if ifc.get("operstate").and_then(|x| x.as_str()) == Some("DOWN") {
            continue;
        }
        let Some(addrs) = ifc.get("addr_info").and_then(|x| x.as_array()) else {
            continue;
        };
        for a in addrs {
            let family = a.get("family").and_then(|x| x.as_str()).unwrap_or("");
            let local = a.get("local").and_then(|x| x.as_str()).unwrap_or("");
            let scope = a.get("scope").and_then(|x| x.as_str()).unwrap_or("");
            if scope == "host" {
                continue;
            }
            if family != "inet" && family != "inet6" {
                continue;
            }
            let Ok(ip) = local.parse::<IpAddr>() else {
                continue;
            };
            if ip.is_loopback() {
                continue;
            }
            if let IpAddr::V6(v6) = ip {
                // Link-local IPv6 isn't useful as a connection target.
                if v6.segments()[0] == 0xfe80 {
                    continue;
                }
            }
            result.push(Iface {
                name: name.clone(),
                addr: ip,
            });
        }
    }
    result
}

pub fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "wayvnc-host".into())
}
