//! Mycelium mini-app developer CLI (Cell C4).

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Parser, Subcommand};
use mycelium_app::miniapp::reproducible_build::{content_attestation_hash, digest_file_hex};
use mycelium_app::miniapp::{scan_bundle, MiniAppBundle, ReproducibleBuild};

#[derive(Parser)]
#[command(name = "miniapp-sdk", about = "Lint and hash Mycelium .mxa bundles")]
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
    /// Emit reproducible-build fields for `mycelium-app.json` (H23).
    Attest {
        path: PathBuf,
        /// Path to RECIPE.md or build instructions file.
        #[arg(long)]
        recipe: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Lint { path } => lint_bundle(&path),
        Command::Hash { path } => hash_bundle(&path),
        Command::Attest { path, recipe } => attest_bundle(&path, &recipe),
    }
}

fn read_bundle(path: &PathBuf) -> anyhow::Result<(Vec<u8>, MiniAppBundle)> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let bundle = MiniAppBundle::load_from_bytes(&bytes)?;
    Ok((bytes, bundle))
}

fn lint_bundle(path: &PathBuf) -> anyhow::Result<()> {
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
    println!("  permissions: {:?}", bundle.manifest.permissions);
    Ok(())
}

fn attest_bundle(path: &PathBuf, recipe: &Path) -> anyhow::Result<()> {
    let (_bytes, bundle) = read_bundle(path)?;
    let content_hash = content_attestation_hash(&bundle)?;
    let archive_hash = bundle_hash_hex(&_bytes);
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
