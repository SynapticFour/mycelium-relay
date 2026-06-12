// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
//! Content-Security-Policy helpers (H05 / P3).

/// `script-src` fragment: strict nonce-only, or nonce + `unsafe-inline` for legacy bundles.
pub fn script_src_attr(script_nonce: &str, allow_inline_scripts: bool) -> String {
    if allow_inline_scripts {
        format!("'nonce-{script_nonce}' 'unsafe-inline'")
    } else {
        format!("'nonce-{script_nonce}'")
    }
}

pub fn meta_tag(script_nonce: &str, allow_inline_scripts: bool) -> String {
    let script_src = script_src_attr(script_nonce, allow_inline_scripts);
    format!(
        r#"<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src {script_src}; style-src 'unsafe-inline'; img-src data:; connect-src 'none'; form-action 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; wasm-unsafe-eval 'none';">"#
    )
}
