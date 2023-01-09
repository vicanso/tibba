use axum::{Json, Router};

use crate::error::HTTPResult;

mod common;
mod user;

pub type JSONResult<T> = HTTPResult<Json<T>>;

pub fn new_router() -> Router {
    let r = Router::new();
    r.merge(common::new_router()).merge(user::new_router())
}
