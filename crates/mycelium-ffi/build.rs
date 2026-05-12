fn main() {
    uniffi::generate_scaffolding("src/mycelium.udl").expect("failed to generate uniffi scaffolding");
}
