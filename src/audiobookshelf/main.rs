#[path = "api.rs"]
pub(crate) mod api;
#[path = "catalog.rs"]
pub(crate) mod catalog;
#[path = "dto.rs"]
pub(crate) mod dto;

pub(crate) use api::router;

#[cfg(test)]
#[path = "../../tests/unit/abs_endpoints.rs"]
mod tests;
