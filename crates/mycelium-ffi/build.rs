// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
fn main() {
    uniffi::generate_scaffolding("src/mycelium.udl")
        .expect("failed to generate uniffi scaffolding");
}
