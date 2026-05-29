use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

pub fn socket_path() -> PathBuf {
    if let Some(rt) = std::env::var_os("XDG_RUNTIME_DIR") {
        PathBuf::from(rt).join("wayvncctl")
    } else {
        PathBuf::from(format!("/tmp/wayvncctl-{}", uid()))
    }
}

fn uid() -> u32 {
    // libc::getuid is a leaf C call with no side effects; cheaper than
    // shelling out to `id -u` and avoids pulling in nix just for this.
    unsafe { libc::getuid() }
}

pub fn is_running() -> bool {
    // The socket file lingers after wayvnc stops -- wayvnc only cleans it up
    // at the next startup ("Deleting stale control socket path"). Existence
    // alone produces false positives, so we also confirm something is
    // listening: a stale UDS file refuses the connect with ECONNREFUSED.
    use std::os::unix::net::UnixStream;
    let path = socket_path();
    path.exists() && UnixStream::connect(&path).is_ok()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Client {
    pub id: String,
    /// Peer IP from inet_ntop on the client socket. Wayvnc does not do
    /// reverse DNS, so this is always a literal address (v4 or v6).
    pub address: Option<String>,
    pub username: Option<String>,
    pub seat: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Output {
    pub name: String,
    #[serde(default)]
    pub captured: bool,
    pub description: Option<String>,
}

pub fn client_list() -> Result<Vec<Client>> {
    Ok(serde_json::from_value(ctl_json(&["client-list"])?).unwrap_or_default())
}

pub fn client_disconnect(id: &str) -> Result<()> {
    ctl_run(&["client-disconnect", id])
}

pub fn output_list() -> Result<Vec<Output>> {
    Ok(serde_json::from_value(ctl_json(&["output-list"])?).unwrap_or_default())
}

pub fn output_set(name: &str) -> Result<()> {
    ctl_run(&["output-set", name])
}

fn ctl_json(args: &[&str]) -> Result<serde_json::Value> {
    let out = Command::new("wayvncctl")
        .arg("--json")
        .args(args)
        .output()
        .context("spawning wayvncctl")?;
    if !out.status.success() {
        return Err(anyhow!(
            "wayvncctl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    if text.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(text.as_ref()).context("parsing wayvncctl json output")
}

fn ctl_run(args: &[&str]) -> Result<()> {
    let out = Command::new("wayvncctl")
        .args(args)
        .output()
        .context("spawning wayvncctl")?;
    if !out.status.success() {
        return Err(anyhow!(
            "wayvncctl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}
