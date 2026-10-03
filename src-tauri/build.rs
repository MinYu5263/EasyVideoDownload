fn main() {
    println!("cargo:rerun-if-changed=target/generated-icons");
    if !std::path::Path::new("target/generated-icons").is_dir() {
        panic!("desktop icons are missing; run `pnpm icons` in the project root first");
    }
    tauri_build::build()
}
