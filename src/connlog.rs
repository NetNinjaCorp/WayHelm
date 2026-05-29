use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

pub fn log_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/state")
        })
        .join("wayhelm")
}

pub fn log_path() -> PathBuf {
    log_dir().join("connections.log")
}

/// Snapshot of a client session, captured at the moment we first see it
/// in `wayvncctl client-list`. Used both for the active-set tracker and
/// for writing log lines.
#[derive(Debug, Clone)]
pub struct ConnInfo {
    pub id: String,
    pub address: Option<String>,
    pub username: Option<String>,
    pub started_at: SystemTime,
}

pub fn append_connect(c: &ConnInfo) -> Result<()> {
    write_line(format_line("CONNECT", c, None))
}

pub fn append_disconnect(c: &ConnInfo) -> Result<()> {
    let duration = c.started_at.elapsed().ok();
    write_line(format_line("DISCONNECT", c, duration))
}

fn format_line(event: &str, c: &ConnInfo, duration: Option<Duration>) -> String {
    let ts = local_timestamp();
    let user = c.username.as_deref().unwrap_or("?");
    let addr = c.address.as_deref().unwrap_or("?");
    match duration {
        Some(d) => format!(
            "{ts}  {event:<11} client={id} user={user} addr={addr} duration={dur}\n",
            id = c.id,
            dur = format_duration(d)
        ),
        None => format!(
            "{ts}  {event:<11} client={id} user={user} addr={addr}\n",
            id = c.id
        ),
    }
}

fn write_line(line: String) -> Result<()> {
    let path = log_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("creating wayhelm state dir")?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

fn local_timestamp() -> String {
    // libc::localtime_r is cheap and avoids pulling chrono just for a
    // human-readable wall-clock string.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&secs, &mut tm);
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m{}s", s / 60, s % 60)
    } else {
        format!("{}h{}m", s / 3600, (s % 3600) / 60)
    }
}

/// Read the trailing `lines` lines of the log file, or a placeholder if it
/// doesn't exist yet. Reads the whole file each time -- fine until logs grow
/// past a few MB, well past anything a personal VNC setup will produce.
pub fn tail(lines: usize) -> String {
    let Ok(body) = std::fs::read_to_string(log_path()) else {
        return String::from("(no log yet)");
    };
    let collected: Vec<&str> = body.lines().collect();
    let start = collected.len().saturating_sub(lines);
    collected[start..].join("\n")
}
