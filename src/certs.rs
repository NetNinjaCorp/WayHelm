use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};

use crate::{config, netinfo};

#[derive(Debug, Clone)]
pub struct CertPaths {
    pub cert: PathBuf,
    pub key: PathBuf,
    pub rsa_key: PathBuf,
}

impl CertPaths {
    pub fn default_paths() -> Self {
        let dir = config::config_dir();
        Self {
            cert: dir.join("tls_cert.pem"),
            key: dir.join("tls_key.pem"),
            rsa_key: dir.join("rsa_key.pem"),
        }
    }
}

/// Generate a fresh self-signed TLS cert (rsa:2048) and a separate RSA key
/// used by wayvnc's RSA-AES auth path. Writes into ~/.config/wayvnc/.
pub fn generate(cn: &str, days: u32) -> Result<CertPaths> {
    let dir = config::config_dir();
    std::fs::create_dir_all(&dir).context("creating wayvnc config dir")?;
    let paths = CertPaths::default_paths();

    let subject = format!("/CN={}", cn);
    // subjectAltName entries: modern clients (and most browsers / wrappers)
    // ignore CN and require SAN. Include the hostname plus every routable
    // local IP so the cert matches whichever endpoint the user connects to.
    let mut san_parts: Vec<String> = vec![format!("DNS:{cn}"), "IP:127.0.0.1".into()];
    for iface in netinfo::local_addresses() {
        san_parts.push(format!("IP:{}", iface.addr));
    }
    let san_ext = format!("subjectAltName={}", san_parts.join(","));

    run_ok(Command::new("openssl").args([
        "req",
        "-x509",
        "-nodes",
        "-newkey",
        "rsa:2048",
        "-keyout",
    ]).arg(&paths.key).args([
        "-out",
    ]).arg(&paths.cert).args([
        "-days",
        &days.to_string(),
        "-subj",
        &subject,
        "-addext",
        &san_ext,
    ]))?;

    // neatvnc's nettle backend only accepts PKCS#1 PEM ("BEGIN RSA PRIVATE KEY").
    // On openssl 3.x both `genrsa` and `genpkey` default to PKCS#8 ("BEGIN PRIVATE
    // KEY") output, which neatvnc rejects with "Unsupported RSA private key format"
    // -- so `-traditional` is required to force the PKCS#1 envelope.
    run_ok(Command::new("openssl").args([
        "genrsa",
        "-traditional",
        "-out",
    ]).arg(&paths.rsa_key).arg("3072"))?;

    set_perm(&paths.key, 0o600)?;
    set_perm(&paths.rsa_key, 0o600)?;
    set_perm(&paths.cert, 0o644)?;
    Ok(paths)
}

pub fn fingerprint(cert: &Path) -> Result<String> {
    let out = Command::new("openssl")
        .args(["x509", "-noout", "-fingerprint", "-sha256", "-in"])
        .arg(cert)
        .output()
        .context("running openssl x509")?;
    if !out.status.success() {
        return Err(anyhow!(
            "openssl x509 failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    // "sha256 Fingerprint=AA:BB:..."
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(s)
}

pub fn not_after(cert: &Path) -> Result<String> {
    let out = Command::new("openssl")
        .args(["x509", "-noout", "-enddate", "-in"])
        .arg(cert)
        .output()
        .context("running openssl x509 -enddate")?;
    if !out.status.success() {
        return Err(anyhow!(
            "openssl x509 -enddate failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    // "notAfter=May 28 12:34:56 2027 GMT"
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(s.trim_start_matches("notAfter=").to_string())
}

fn run_ok(cmd: &mut Command) -> Result<()> {
    let out = cmd.output().context("spawning openssl")?;
    if !out.status.success() {
        return Err(anyhow!(
            "openssl failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn set_perm(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("chmod {:o} {}", mode, path.display()))?;
    Ok(())
}
