// rust-embed needs the folder to exist at compile time. Create an empty one so
// `cargo build` works without Node; the server then shows "UI not built".
fn main() {
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    if !dist.exists() {
        let _ = std::fs::create_dir_all(&dist);
    }
    println!("cargo:rerun-if-changed=../../web/dist");
}
