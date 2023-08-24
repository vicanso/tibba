use crate::error::{HttpError, HttpResult};
use crate::state::AppState;
use axum::{extract::State, http::Request, middleware::Next, response::Response};

pub async fn processing_limit<B>(
    State(state): State<&AppState>,
    req: Request<B>,
    next: Next<B>,
) -> HttpResult<Response> {
    if state.increase_processing() > state.processing_limit && state.processing_limit != 0 {
        state.decrease_processing();
        return Err(HttpError::new_with_status("Too Many Requests", 429));
    }
    let resp = next.run(req).await;
    state.decrease_processing();
    Ok(resp)
}
