#[path = "api.rs"]
pub(crate) mod api;
#[path = "audible.rs"]
pub(crate) mod audible;
#[path = "catalog.rs"]
pub(crate) mod catalog;
#[path = "dto.rs"]
pub(crate) mod dto;
#[path = "matching.rs"]
pub(crate) mod matching;
#[path = "providers.rs"]
pub(crate) mod providers;
#[path = "scanner.rs"]
pub(crate) mod scanner;
#[path = "socket.rs"]
pub(crate) mod socket;

pub(crate) use api::router;

#[cfg(test)]
#[path = "../../tests/unit/abs_endpoints.rs"]
mod tests;
