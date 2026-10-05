fn main() {
    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows_msvc {
        // Native dialog tests link TaskDialogIndirect too. Library test runners
        // need Common Controls v6, just as Tauri's application binary does.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
    }
    println!("cargo:rerun-if-changed=target/generated-icons");
    if !std::path::Path::new("target/generated-icons").is_dir() {
        panic!("desktop icons are missing; run `pnpm icons` in the project root first");
    }
    tauri_build::build();
    if windows_msvc {
        // Tauri already embeds the application's manifest in resource.lib.
        // Keep the linker-generated manifest for library test runners only;
        // generating another one for the binary duplicates resource ID 1.
        println!("cargo:rustc-link-arg-bin=EasyVideoDownload=/MANIFEST:NO");
    }
}
