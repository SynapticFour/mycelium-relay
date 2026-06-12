// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
use std::collections::HashMap;
use std::io::Read;

use super::manifest::MiniAppManifest;

/// Unpacked mini-app bundle in memory.
#[derive(Debug, Clone)]
pub struct MiniAppBundle {
    pub manifest: MiniAppManifest,
    pub files: HashMap<String, Vec<u8>>,
}

fn is_safe_bundle_path(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    if name.contains("..") {
        return false;
    }
    for seg in name.split(['/', '\\']) {
        if seg == ".." {
            return false;
        }
    }
    true
}

impl MiniAppBundle {
    pub fn load_from_file(path: &str) -> anyhow::Result<Self> {
        let data = std::fs::read(path)?;
        Self::load_from_bytes(&data)
    }

    pub fn load_from_bytes(data: &[u8]) -> anyhow::Result<Self> {
        let cursor = std::io::Cursor::new(data);
        let mut zip = zip::ZipArchive::new(cursor)?;
        let mut files = HashMap::new();

        for i in 0..zip.len() {
            let mut file = zip.by_index(i)?;
            if file.is_dir() {
                continue;
            }
            let name = file.name().to_string();
            if !is_safe_bundle_path(&name) {
                anyhow::bail!("invalid file path in bundle: {name}");
            }
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            files.insert(name, buf);
        }

        let manifest_bytes = files
            .get("mycelium-app.json")
            .ok_or_else(|| anyhow::anyhow!("missing mycelium-app.json"))?;
        let manifest: MiniAppManifest = serde_json::from_slice(manifest_bytes)?;
        manifest.validate()?;

        Ok(Self { manifest, files })
    }

    pub fn get_file(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(|v| v.as_slice())
    }

    pub fn entry_html(&self) -> anyhow::Result<&str> {
        let bytes = self
            .get_file(&self.manifest.entry)
            .ok_or_else(|| anyhow::anyhow!("entry point not found: {}", self.manifest.entry))?;
        std::str::from_utf8(bytes).map_err(|_| anyhow::anyhow!("entry point is not valid UTF-8"))
    }

    pub fn total_size(&self) -> usize {
        self.files.values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::FileOptions;

    fn sample_manifest_json() -> String {
        serde_json::json!({
            "id": "com.test.app",
            "name": "Test App",
            "description": "A test",
            "version": "0.1.0",
            "developer": "Test",
            "entry": "index.html",
            "permissions": [],
            "min_mycelium_version": "0.1.0",
            "accepts_payments": false,
            "categories": [],
        })
        .to_string()
    }

    #[test]
    fn bundle_load_and_validate() {
        let mut zip_buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let opts = FileOptions::default();
            zip.start_file("mycelium-app.json", opts).unwrap();
            zip.write_all(sample_manifest_json().as_bytes()).unwrap();
            zip.start_file("index.html", opts).unwrap();
            zip.write_all(b"<h1>Test</h1>").unwrap();
            zip.finish().unwrap();
        }

        let bundle = MiniAppBundle::load_from_bytes(&zip_buf).unwrap();
        assert_eq!(bundle.manifest.id, "com.test.app");
        assert_eq!(bundle.entry_html().unwrap(), "<h1>Test</h1>");
    }

    #[test]
    fn wasm_runtime_rejected() {
        let mut manifest: serde_json::Value =
            serde_json::from_str(&sample_manifest_json()).unwrap();
        manifest["runtime"] = serde_json::json!("wasm");
        let mut zip_buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let opts = FileOptions::default();
            zip.start_file("mycelium-app.json", opts).unwrap();
            zip.write_all(manifest.to_string().as_bytes()).unwrap();
            zip.start_file("index.html", opts).unwrap();
            zip.write_all(b"<h1>Test</h1>").unwrap();
            zip.finish().unwrap();
        }
        assert!(MiniAppBundle::load_from_bytes(&zip_buf).is_err());
    }

    #[test]
    fn path_traversal_rejected() {
        let mut zip_buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let opts = FileOptions::default();
            zip.start_file("mycelium-app.json", opts).unwrap();
            zip.write_all(sample_manifest_json().as_bytes()).unwrap();
            zip.start_file("../evil.txt", opts).unwrap();
            zip.write_all(b"x").unwrap();
            zip.finish().unwrap();
        }

        assert!(MiniAppBundle::load_from_bytes(&zip_buf).is_err());
    }

    #[test]
    fn bundle_size_limit_on_install() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let store_path = dir.path().join("miniapp_db");
        let store = crate::miniapp::store::AppStore::open(store_path.to_str().unwrap()).unwrap();

        let big = vec![b'a'; 11 * 1024 * 1024];
        let mut zip_buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut zip_buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let opts = FileOptions::default();
            zip.start_file("mycelium-app.json", opts).unwrap();
            zip.write_all(sample_manifest_json().as_bytes()).unwrap();
            zip.start_file("big.bin", opts).unwrap();
            zip.write_all(&big).unwrap();
            zip.finish().unwrap();
        }

        assert!(store.install(&zip_buf).is_err());
    }
}
