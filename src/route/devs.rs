use crate::utils::query::dev;
use axum::Json;

pub async fn devs_router() -> Json<Vec<String>> {
    dev::devs_sorted().into()
}
// Device listing endpoints
