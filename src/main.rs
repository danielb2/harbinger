#![recursion_limit = "256"]

#[path = "harbringer/main.rs"]
mod harbringer;
#[path = "plex/main.rs"]
mod plex;

#[cfg(test)]
pub(crate) use harbringer::{metadata_scan_assets_only, scan_library_inventory};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    harbringer::run().await
}
