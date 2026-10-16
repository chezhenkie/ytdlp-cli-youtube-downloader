//! Differential harness dispatcher: one arm per ported function, the
//! scripted world they share, and the case-to-record projections; all
//! of it lives in arms.rs. main.rs calls dispatch::dispatch only.

use crate::json::Json;

mod arms;

pub fn dispatch(name: &str, args: &[Json]) -> Result<Json, String> {
    arms::dispatch(name, args)
}
