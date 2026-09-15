fn main() {
    println!("cargo:rerun-if-changed=migrations");
    #[cfg(feature = "desktop")]
    tauri_build::build();
}
