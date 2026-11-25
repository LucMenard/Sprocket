fn main() {
    // The exe embeds icons/icon.ico (and the window icon); without this, a new icon is not picked up
    // until the crate is rebuilt from scratch
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    tauri_build::build()
}
