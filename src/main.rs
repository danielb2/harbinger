#![recursion_limit = "256"]

#[path = "harbinger/main.rs"]
mod harbinger;
#[path = "plex/main.rs"]
mod plex;

#[cfg(test)]
pub(crate) use harbinger::{metadata_scan_assets_only, scan_library_inventory};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    harbinger::run().await
}
