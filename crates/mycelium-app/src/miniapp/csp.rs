// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Content-Security-Policy helpers (H05 / P3).

/// `script-src` fragment: strict nonce-only (SD-040 — no `unsafe-inline`).
pub fn script_src_attr(script_nonce: &str) -> String {
    format!("'nonce-{script_nonce}'")
}

pub fn meta_tag(script_nonce: &str) -> String {
    let script_src = script_src_attr(script_nonce);
    format!(
        r#"<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src {script_src}; style-src 'unsafe-inline'; img-src data:; connect-src 'none'; form-action 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; wasm-unsafe-eval 'none';">"#
    )
}
