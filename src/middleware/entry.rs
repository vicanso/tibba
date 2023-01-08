use axum::{http::Request, middleware::Next, response::Response};

use crate::util::{
    random_string, set_header_if_not_exist, set_no_cache_if_not_exist, ACCOUNT, DEVICE_ID, TRACE_ID,
};

pub async fn entry<B>(req: Request<B>, next: Next<B>) -> Response {
    let trace_id = random_string(8);
    ACCOUNT
        .scope("".to_string(), async {
            TRACE_ID
                .scope(trace_id.clone(), async {
                    let mut resp = next.run(req).await;
                    let headers = resp.headers_mut();
                    set_no_cache_if_not_exist(headers);
                    // 忽略出错
                    let _ = set_header_if_not_exist(headers, "X-Trace-Id".to_string(), trace_id);

                    resp
                })
                .await
        })
        .await
}
