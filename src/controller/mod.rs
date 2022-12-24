use axum::Router;

mod user;

pub fn new_router() -> Router {
    let r = Router::new();
    r.merge(user::new_router())
}
