use axum::{http::Request, middleware::Next, response::Response};

use crate::util::{
    random_string, set_context, set_header_if_not_exist, set_no_cache_if_not_exist, Context,
};

pub async fn entry<B>(mut req: Request<B>, next: Next<B>) -> Response {
    let trace_id = random_string(8);
    let mut ctx = Context::new();
    ctx.trace_id = trace_id.clone();
    set_context(req.extensions_mut(), ctx);

    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();
    set_no_cache_if_not_exist(headers);
    // 忽略出错
    let _ = set_header_if_not_exist(headers, "X-Trace-Id".to_string(), trace_id);

    resp
}
