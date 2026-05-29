use std::io::Read;

/// Generate a random alphanumeric password using /dev/urandom.
/// Excludes visually-confusable characters (0/O, 1/l/I) to make
/// hand-transcription less error-prone.
pub fn random_password(n: usize) -> String {
    const ALPHA: &[u8] =
        b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut buf = vec![0u8; n];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    }
    buf.iter()
        .map(|b| ALPHA[(*b as usize) % ALPHA.len()] as char)
        .collect()
}
