use crate::error::{HttpError, HttpResult};
use crate::state::AppState;
use axum::{body::Body, extract::State, http::Request, middleware::Next, response::Response};

pub async fn processing_limit(
    State(state): State<&AppState>,
    req: Request<Body>,
    next: Next,
) -> HttpResult<Response> {
    if state.increase_processing() > state.processing_limit && state.processing_limit != 0 {
        state.decrease_processing();
        return Err(HttpError::new_with_status("Too Many Requests", 429));
    }
    let resp = next.run(req).await;
    state.decrease_processing();
    Ok(resp)
}
