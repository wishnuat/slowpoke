//! The network half: probe each hop (Wi-Fi, router, internet, DNS) and blame the first unhealthy one.

pub mod history;
pub mod monitor;
pub mod probe;
pub mod verdict;
