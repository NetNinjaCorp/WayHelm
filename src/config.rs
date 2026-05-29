use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub address: Option<String>,
    pub port: Option<u16>,
    pub enable_auth: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub certificate_file: Option<PathBuf>,
    pub private_key_file: Option<PathBuf>,
    pub rsa_private_key_file: Option<PathBuf>,
    pub use_relative_paths: bool,
    pub relax_encryption: bool,
    pub xkb_layout: Option<String>,
    pub xkb_variant: Option<String>,
    pub xkb_model: Option<String>,
    pub xkb_options: Option<String>,
    pub xkb_rules: Option<String>,
    /// Unknown / pass-through keys we found in the file but don't model.
    pub extra: BTreeMap<String, String>,
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("wayvnc")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config")
}

impl Config {
    pub fn load_or_default() -> Result<Self> {
        let path = config_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut c = Config::default();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (k, v) = line
                .split_once('=')
                .with_context(|| format!("line {}: missing '='", i + 1))?;
            let k = k.trim();
            let v = v.trim();
            match k {
                "address" => c.address = Some(v.into()),
                "port" => {
                    c.port = Some(
                        v.parse()
                            .with_context(|| format!("line {}: bad port '{}'", i + 1, v))?,
                    )
                }
                "enable_auth" => c.enable_auth = parse_bool(v),
                "username" => c.username = Some(v.into()),
                "password" => c.password = Some(v.into()),
                "certificate_file" => c.certificate_file = Some(PathBuf::from(v)),
                "private_key_file" => c.private_key_file = Some(PathBuf::from(v)),
                "rsa_private_key_file" => c.rsa_private_key_file = Some(PathBuf::from(v)),
                "use_relative_paths" => c.use_relative_paths = parse_bool(v),
                "relax_encryption" => c.relax_encryption = parse_bool(v),
                "xkb_layout" => c.xkb_layout = Some(v.into()),
                "xkb_variant" => c.xkb_variant = Some(v.into()),
                "xkb_model" => c.xkb_model = Some(v.into()),
                "xkb_options" => c.xkb_options = Some(v.into()),
                "xkb_rules" => c.xkb_rules = Some(v.into()),
                other => {
                    c.extra.insert(other.to_string(), v.to_string());
                }
            }
        }
        Ok(c)
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        // Strip CR/LF from every value at the write boundary so user input that
        // contains a newline can't inject a second key=value line (and through
        // it, sensitive keys like `password=` or `enable_auth=false`). This is
        // defense in depth; values normally arrive from controlled UI paths.
        let mut push = |k: &str, v: &str| {
            s.push_str(k);
            s.push('=');
            for c in v.chars() {
                if c != '\n' && c != '\r' {
                    s.push(c);
                }
            }
            s.push('\n');
        };
        if let Some(v) = &self.address {
            push("address", v);
        }
        if let Some(v) = &self.port {
            push("port", &v.to_string());
        }
        push("enable_auth", if self.enable_auth { "true" } else { "false" });
        if let Some(v) = &self.username {
            push("username", v);
        }
        if let Some(v) = &self.password {
            push("password", v);
        }
        if let Some(v) = &self.certificate_file {
            push("certificate_file", &v.display().to_string());
        }
        if let Some(v) = &self.private_key_file {
            push("private_key_file", &v.display().to_string());
        }
        if let Some(v) = &self.rsa_private_key_file {
            push("rsa_private_key_file", &v.display().to_string());
        }
        if self.use_relative_paths {
            push("use_relative_paths", "true");
        }
        if self.relax_encryption {
            push("relax_encryption", "true");
        }
        if let Some(v) = &self.xkb_layout {
            push("xkb_layout", v);
        }
        if let Some(v) = &self.xkb_variant {
            push("xkb_variant", v);
        }
        if let Some(v) = &self.xkb_model {
            push("xkb_model", v);
        }
        if let Some(v) = &self.xkb_options {
            push("xkb_options", v);
        }
        if let Some(v) = &self.xkb_rules {
            push("xkb_rules", v);
        }
        for (k, v) in &self.extra {
            push(k, v);
        }
        s
    }

    /// Render a copy of the config with the password masked so it's safe to display.
    pub fn render_redacted(&self) -> String {
        let mut copy = self.clone();
        if copy.password.is_some() {
            copy.password = Some("••••••••".into());
        }
        copy.render()
    }

    pub fn save(&self) -> Result<()> {
        let dir = config_dir();
        fs::create_dir_all(&dir).context("creating wayvnc config dir")?;
        let path = config_path();
        write_atomic(&path, &self.render(), 0o600)
    }

    /// Heuristic: has the user finished the secure-setup wizard?
    pub fn is_configured(&self) -> bool {
        self.enable_auth
            && self.username.is_some()
            && self.password.is_some()
            && self.certificate_file.is_some()
            && self.private_key_file.is_some()
    }
}

fn parse_bool(v: &str) -> bool {
    matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "1" | "on")
}

#[cfg(unix)]
fn write_atomic(path: &Path, body: &str, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let tmp = path.with_extension("tmp");
    let mut f = fs::File::create(&tmp)
        .with_context(|| format!("creating {}", tmp.display()))?;
    f.set_permissions(fs::Permissions::from_mode(mode))?;
    f.write_all(body.as_bytes())?;
    f.sync_all().ok();
    fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}
