#[path = "harbringer/main.rs"]
mod app;

pub(super) use app::run;
#[cfg(test)]
pub(crate) use app::{metadata_scan_assets_only, scan_library_inventory};
