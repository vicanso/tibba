use axum::{Json, Router};

use crate::error::HTTPError;

mod user;

pub type JSONResult<T> = Result<Json<T>, HTTPError>;

pub fn new_router() -> Router {
    let r = Router::new();
    r.merge(user::new_router())
}
