// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Mycelium mini-app developer CLI (lint, pack, sign, hash).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand};
use libp2p::identity::Keypair;
use mycelium_app::miniapp::bundle_signature::{self, BundleSignatureFile, BUNDLE_SIG_FILE};
use mycelium_app::miniapp::manifest::MiniAppManifest;
use mycelium_app::miniapp::reproducible_build::{content_attestation_hash, digest_file_hex};
use mycelium_app::miniapp::store::{AppSource, AppStoreListing};
use mycelium_app::miniapp::{scan_bundle, MiniAppBundle, ReproducibleBuild};
use zip::write::FileOptions;
use zip::ZipArchive;

#[derive(Parser)]
#[command(
    name = "miniapp-sdk",
    about = "Lint, pack, and sign Mycelium .mxa bundles"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate bundle structure, manifest schema, and security policy.
    Lint { path: PathBuf },
    /// Print BLAKE3 hash of the bundle bytes (matches install preview).
    Hash { path: PathBuf },
    /// Build a .mxa zip from a mini-app source directory.
    Pack {
        /// Directory containing mycelium-app.json and assets.
        dir: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Add mycelium-bundle.sig.json (and optional developer_peer_id) to a bundle.
    Sign {
        path: PathBuf,
        /// Ed25519 identity file (libp2p protobuf secret, same as node identity).
        #[arg(long)]
        identity: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Pack + sign in one step.
    PackSign {
        dir: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        identity: PathBuf,
    },
    /// Generate an Ed25519 libp2p identity for bundle signing.
    Keygen {
        /// Output directory (writes official.ed25519 + official.peer-id).
        dir: PathBuf,
    },
    /// Emit signed store listing JSON for a .mxa bundle.
    Listing {
        path: PathBuf,
        #[arg(long)]
        identity: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Write bundled_listings.json from signed .mxa files in a directory.
    Listings {
        dir: PathBuf,
        #[arg(long)]
        identity: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Emit reproducible-build fields for `mycelium-app.json` (H23).
    Attest {
        path: PathBuf,
        #[arg(long)]
        recipe: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Lint { path } => lint_bundle(&path),
        Command::Hash { path } => hash_bundle(&path),
        Command::Pack { dir, output } => pack_dir(&dir, &output),
        Command::Sign {
            path,
            identity,
            output,
        } => sign_bundle(&path, &identity, output.as_ref()),
        Command::PackSign {
            dir,
            output,
            identity,
        } => {
            let tmp = tempfile::NamedTempFile::new()?;
            pack_dir(&dir, tmp.path())?;
            sign_bundle(tmp.path(), &identity, Some(&output))?;
            println!("wrote signed bundle: {}", output.display());
            Ok(())
        }
        Command::Keygen { dir } => keygen(&dir),
        Command::Listing {
            path,
            identity,
            output,
        } => emit_listing(&path, &identity, output.as_ref()),
        Command::Listings {
            dir,
            identity,
            output,
        } => emit_listings_dir(&dir, &identity, &output),
        Command::Attest { path, recipe } => attest_bundle(&path, &recipe),
    }
}

fn read_bundle(path: &Path) -> anyhow::Result<(Vec<u8>, MiniAppBundle)> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let bundle = MiniAppBundle::load_from_bytes(&bytes)?;
    Ok((bytes, bundle))
}

fn lint_bundle(path: &Path) -> anyhow::Result<()> {
    let (bytes, bundle) = read_bundle(path)?;
    if bundle.total_size() > 10 * 1024 * 1024 {
        anyhow::bail!("bundle exceeds 10 MB limit");
    }
    if bundle.manifest.runtime != "webview" {
        anyhow::bail!(
            "unsupported runtime {:?} (only webview is supported)",
            bundle.manifest.runtime
        );
    }
    bundle
        .entry_html()
        .context("entry HTML must be valid UTF-8")?;
    scan_bundle(&bundle, &bytes).context("bundle security scan failed")?;
    let sig_ok = bundle_signature::verify_bundle_developer_signature(&bundle, &bytes)?;
    let hash = bundle_hash_hex(&bytes);
    let content_hash = content_attestation_hash(&bundle)?;
    if let Some(rb) = &bundle.manifest.reproducible_build {
        rb.validate()
            .context("invalid reproducible_build in manifest")?;
        if let Some(attested) = &rb.attested_bundle_hash {
            if !attested.eq_ignore_ascii_case(&content_hash) {
                anyhow::bail!(
                    "reproducible_build.attested_bundle_hash mismatch (manifest {attested} vs content {content_hash})"
                );
            }
            println!("  reproducible: attested hash matches content attestation");
        } else {
            println!("  reproducible: declared but attested_bundle_hash missing");
        }
    }
    println!(
        "ok: {} v{} ({} bytes)",
        bundle.manifest.id,
        bundle.manifest.version,
        bytes.len()
    );
    println!("  hash: {hash}");
    println!("  bundle_signature_ok: {sig_ok}");
    println!("  permissions: {:?}", bundle.manifest.permissions);
    Ok(())
}

fn pack_dir(dir: &Path, output: &Path) -> anyhow::Result<()> {
    let manifest_path = dir.join("mycelium-app.json");
    if !manifest_path.is_file() {
        anyhow::bail!("missing mycelium-app.json in {}", dir.display());
    }
    let manifest: MiniAppManifest =
        serde_json::from_slice(&std::fs::read(&manifest_path)?).context("parse manifest")?;
    manifest.validate()?;

    let mut zip_buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
        let opts = FileOptions::default();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .context("bad filename")?;
            if name.ends_with(".mxa") || name == BUNDLE_SIG_FILE {
                continue;
            }
            let mut f = std::fs::File::open(&path)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            writer.start_file(name, opts)?;
            writer.write_all(&buf)?;
        }
        writer.finish()?;
    }

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, &zip_buf)?;
    println!(
        "packed {} v{} → {} ({} bytes)",
        manifest.id,
        manifest.version,
        output.display(),
        zip_buf.len()
    );
    Ok(())
}

fn emit_listing(
    bundle_path: &Path,
    identity: &Path,
    output: Option<&PathBuf>,
) -> anyhow::Result<()> {
    let listing = build_listing(bundle_path, identity)?;
    let json = serde_json::to_string_pretty(&listing)?;
    if let Some(out) = output {
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(out, &json)?;
        println!("wrote listing → {}", out.display());
    } else {
        println!("{json}");
    }
    Ok(())
}

fn emit_listings_dir(dir: &Path, identity: &Path, output: &Path) -> anyhow::Result<()> {
    let mut listings = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mxa") {
            continue;
        }
        listings.push(build_listing(&path, identity)?);
    }
    listings.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    let file = serde_json::json!({ "listings": listings });
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, serde_json::to_string_pretty(&file)?)?;
    println!("wrote {} listing(s) → {}", listings.len(), output.display());
    Ok(())
}

fn build_listing(bundle_path: &Path, identity: &Path) -> anyhow::Result<AppStoreListing> {
    let keypair = load_keypair(identity)?;
    let peer_id = keypair.public().to_peer_id().to_string();
    let bundle_bytes = std::fs::read(bundle_path)?;
    let bundle = MiniAppBundle::load_from_bytes(&bundle_bytes)?;
    let mut manifest = bundle.manifest.clone();
    if manifest.developer_peer_id.is_none() {
        manifest.developer_peer_id = Some(peer_id.clone());
    }
    AppStoreListing::new_signed(
        manifest,
        &bundle_bytes,
        vec![AppSource::Peer(peer_id)],
        &keypair,
    )
}

fn keygen(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let keypair = Keypair::generate_ed25519();
    let peer_id = keypair.public().to_peer_id();
    let enc = keypair.to_protobuf_encoding()?;
    let key_path = dir.join("official.ed25519");
    let peer_path = dir.join("official.peer-id");
    std::fs::write(&key_path, &enc)?;
    std::fs::write(&peer_path, peer_id.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
    }
    println!("wrote {}", key_path.display());
    println!("peer_id: {peer_id}");
    Ok(())
}

fn load_keypair(path: &Path) -> anyhow::Result<Keypair> {
    let bytes = std::fs::read(path)?;
    Keypair::from_protobuf_encoding(&bytes).context("identity file must be libp2p protobuf Ed25519")
}

fn sign_bundle(path: &Path, identity: &Path, output: Option<&PathBuf>) -> anyhow::Result<()> {
    let keypair = load_keypair(identity)?;
    let peer_id = keypair.public().to_peer_id().to_string();
    let bytes = std::fs::read(path)?;

    let mut archive = ZipArchive::new(std::io::Cursor::new(&bytes))?;
    let mut manifest: MiniAppManifest = {
        let mut f = archive.by_name("mycelium-app.json")?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        serde_json::from_slice(&buf)?
    };
    if manifest.developer_peer_id.is_none() {
        manifest.developer_peer_id = Some(peer_id.clone());
    }

    let mut unsigned_with_manifest = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut unsigned_with_manifest));
        let opts = FileOptions::default();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name().to_string();
            if name == BUNDLE_SIG_FILE {
                continue;
            }
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            if name == "mycelium-app.json" {
                buf = serde_json::to_vec_pretty(&manifest)?;
            }
            writer.start_file(name, opts)?;
            writer.write_all(&buf)?;
        }
        writer.finish()?;
    }

    let sig = BundleSignatureFile::sign(&unsigned_with_manifest, &keypair)?;
    let sig_json = serde_json::to_string_pretty(&sig)?;

    let mut out = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        let opts = FileOptions::default();
        let mut archive2 = ZipArchive::new(std::io::Cursor::new(&unsigned_with_manifest))?;
        for i in 0..archive2.len() {
            let mut file = archive2.by_index(i)?;
            let name = file.name().to_string();
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            writer.start_file(name, opts)?;
            writer.write_all(&buf)?;
        }
        writer.start_file(BUNDLE_SIG_FILE, opts)?;
        writer.write_all(sig_json.as_bytes())?;
        writer.finish()?;
    }

    let dest = output.map(|p| p.as_path()).unwrap_or(path);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(dest, &out)?;

    let (signed, bundle) = read_bundle(dest)?;
    let ok = bundle_signature::verify_bundle_developer_signature(&bundle, &signed)?;
    println!(
        "signed {} v{} → {} (peer_id={peer_id}, sig_ok={ok})",
        manifest.id,
        manifest.version,
        dest.display()
    );
    Ok(())
}

fn attest_bundle(path: &Path, recipe: &Path) -> anyhow::Result<()> {
    let (bytes, bundle) = read_bundle(path)?;
    let content_hash = content_attestation_hash(&bundle)?;
    let archive_hash = bundle_hash_hex(&bytes);
    let recipe_digest = digest_file_hex(recipe)?;
    let attestation = ReproducibleBuild {
        recipe_url: None,
        recipe_digest_hex: Some(recipe_digest),
        attested_bundle_hash: Some(content_hash.clone()),
        sdk_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };
    attestation.validate()?;
    println!("{}", serde_json::to_string_pretty(&attestation)?);
    println!("# Merge the JSON above into mycelium-app.json under \"reproducible_build\"");
    println!("# content attestation hash: {content_hash}");
    println!("# archive hash (changes when manifest is updated): {archive_hash}");
    Ok(())
}

fn hash_bundle(path: &PathBuf) -> anyhow::Result<()> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    println!("{}", bundle_hash_hex(&bytes));
    Ok(())
}

fn bundle_hash_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
