// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Static bundle analysis at install/lint time (H24 / P4).

use super::bundle::MiniAppBundle;
use super::install_policy::html_has_inline_script;
use super::manifest::MiniAppManifest;

/// Maximum non-directory files in a bundle.
pub const MAX_BUNDLE_FILES: usize = 128;
/// Reject zip bombs when uncompressed total exceeds this multiple of archive size.
pub const MAX_COMPRESSION_RATIO: usize = 32;
/// Minimum uncompressed bytes before ratio check applies.
pub const MIN_BYTES_FOR_RATIO_CHECK: usize = 512 * 1024;

const ALLOWED_EXTENSIONS: &[&str] = &[
    "html", "htm", "css", "js", "json", "png", "jpg", "jpeg", "gif", "svg", "webp", "ico", "txt",
    "woff", "woff2", "ttf", "eot", "map", "md",
];

/// Scan bundle contents and archive metrics. Called from install preview and `miniapp-sdk lint`.
pub fn scan_bundle(bundle: &MiniAppBundle, archive_bytes: &[u8]) -> anyhow::Result<()> {
    if bundle.files.len() > MAX_BUNDLE_FILES {
        anyhow::bail!(
            "bundle has {} files (max {MAX_BUNDLE_FILES})",
            bundle.files.len()
        );
    }

    let uncompressed: usize = bundle.files.values().map(|b| b.len()).sum();
    let compressed = archive_bytes.len().max(1);
    if uncompressed >= MIN_BYTES_FOR_RATIO_CHECK
        && uncompressed > compressed.saturating_mul(MAX_COMPRESSION_RATIO)
    {
        anyhow::bail!(
            "bundle compression ratio suspicious ({uncompressed} bytes uncompressed vs {compressed} archive)"
        );
    }

    for (path, data) in &bundle.files {
        if path == "mycelium-app.json" {
            continue;
        }
        validate_path_extension(path)?;
        sniff_polyglot(path, data)?;
        if path.ends_with(".html") || path.ends_with(".htm") {
            validate_html_asset(path, data)?;
        }
    }

    validate_entry_html(&bundle.manifest, bundle)?;
    Ok(())
}

fn validate_path_extension(path: &str) -> anyhow::Result<()> {
    let lower = path.to_lowercase();
    if lower.contains("..") {
        anyhow::bail!("invalid path: {path}");
    }
    let ext = lower
        .rsplit('.')
        .next()
        .filter(|e| !e.is_empty() && !e.contains('/'));
    let Some(ext) = ext else {
        anyhow::bail!("file without allowed extension: {path}");
    };
    if !ALLOWED_EXTENSIONS.contains(&ext) {
        anyhow::bail!("disallowed file type .{ext} in bundle ({path})");
    }
    Ok(())
}

fn sniff_polyglot(path: &str, data: &[u8]) -> anyhow::Result<()> {
    if data.starts_with(b"MZ") {
        anyhow::bail!("executable content detected in {path}");
    }
    if data.starts_with(b"%PDF-") {
        anyhow::bail!("PDF content not allowed in bundle: {path}");
    }
    if data.len() >= 4 && &data[..4] == b"\x7fELF" {
        anyhow::bail!("ELF binary not allowed in bundle: {path}");
    }
    Ok(())
}

fn validate_html_asset(path: &str, data: &[u8]) -> anyhow::Result<()> {
    let html =
        std::str::from_utf8(data).map_err(|_| anyhow::anyhow!("{path} is not valid UTF-8 HTML"))?;
    validate_html_content(html, path)
}

fn validate_entry_html(manifest: &MiniAppManifest, bundle: &MiniAppBundle) -> anyhow::Result<()> {
    let bytes = bundle
        .get_file(&manifest.entry)
        .ok_or_else(|| anyhow::anyhow!("entry point not found: {}", manifest.entry))?;
    let html = std::str::from_utf8(bytes)
        .map_err(|_| anyhow::anyhow!("entry {} is not valid UTF-8", manifest.entry))?;
    validate_html_content(html, &manifest.entry)?;
    let _ = html_has_inline_script(html);
    Ok(())
}

fn validate_html_content(html: &str, label: &str) -> anyhow::Result<()> {
    let lower = html.to_lowercase();
    for needle in ["javascript:", "vbscript:", "data:text/html", "file://"] {
        if lower.contains(needle) {
            anyhow::bail!("{label} contains forbidden URL scheme ({needle})");
        }
    }
    for token in [
        "\"file:",
        "'file:",
        "(file:",
        " href=\"file:",
        " src=\"file:",
    ] {
        if lower.contains(token) {
            anyhow::bail!("{label} contains forbidden URL scheme (file:)");
        }
    }
    if lower.contains("<base") {
        anyhow::bail!("{label} must not contain <base> (navigation escape risk)");
    }
    if lower.contains("<meta") && lower.contains("http-equiv") && lower.contains("refresh") {
        anyhow::bail!("{label} must not use meta refresh");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::miniapp::manifest::MiniAppManifest;
    use std::collections::HashMap;

    fn minimal_bundle(entry_html: &str) -> (MiniAppBundle, Vec<u8>) {
        let manifest = MiniAppManifest {
            id: "com.test.app".into(),
            name: "T".into(),
            description: "d".into(),
            version: "1.0.0".into(),
            developer: "X".into(),
            developer_peer_id: None,
            entry: "index.html".into(),
            icon_base64: None,
            permissions: vec![],
            min_mycelium_version: "0.1.0".into(),
            accepts_payments: false,
            payment_address: None,
            categories: vec![],
            runtime: "webview".into(),
            bulletin_scopes: vec![],
            reproducible_build: None,
        };
        let files = HashMap::from([("index.html".into(), entry_html.as_bytes().to_vec())]);
        let bundle = MiniAppBundle { manifest, files };
        let archive = b"fake-archive";
        (bundle, archive.to_vec())
    }

    #[test]
    fn accepts_profile_object_key() {
        let (bundle, archive) =
            minimal_bundle("<html><script>const x = { profile: {} };</script></html>");
        scan_bundle(&bundle, &archive).unwrap();
    }

    #[test]
    fn rejects_javascript_url_in_entry() {
        let (bundle, archive) = minimal_bundle(r#"<a href="javascript:alert(1)">x</a>"#);
        assert!(scan_bundle(&bundle, &archive).is_err());
    }

    #[test]
    fn rejects_exe_magic() {
        let (mut bundle, archive) = minimal_bundle("<html></html>");
        bundle
            .files
            .insert("assets/evil.js".into(), b"MZ\x90\x00".to_vec());
        assert!(scan_bundle(&bundle, &archive).is_err());
    }

    #[test]
    fn accepts_minimal_html() {
        let (bundle, archive) = minimal_bundle("<html><body>ok</body></html>");
        scan_bundle(&bundle, &archive).unwrap();
    }
}
