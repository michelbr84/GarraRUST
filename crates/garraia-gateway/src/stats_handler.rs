use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::state::SharedState;

#[derive(Serialize)]
pub struct StatsResponse {
    pub version: &'static str,
    pub uptime_secs: u64,
    pub active_sessions: usize,
    pub gateway_status: &'static str,
}

pub async fn stats_handler(State(state): State<SharedState>) -> Json<StatsResponse> {
    Json(StatsResponse {
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs: state.boot_time.elapsed().as_secs(),
        active_sessions: state.sessions.len(),
        gateway_status: "online",
    })
}
