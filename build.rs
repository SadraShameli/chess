fn main() {
    println!("cargo:rerun-if-changed=capabilities");
    let capabilities = if cfg!(feature = "native-test") {
        "capabilities/*.json"
    } else {
        "capabilities/main.json"
    };
    tauri_build::try_build(tauri_build::Attributes::new().capabilities_path_pattern(capabilities))
        .unwrap_or_else(|error| {
            eprintln!("Desktop build failed: {error}");
            std::process::exit(1);
        });
}
