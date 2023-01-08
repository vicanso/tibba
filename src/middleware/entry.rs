use axum::{http::Request, middleware::Next, response::Response};
use axum_extra::extract::cookie::CookieJar;

use crate::util::{
    get_device_id_from_cookie, random_string, set_header_if_not_exist, set_no_cache_if_not_exist,
    ACCOUNT, DEVICE_ID, TRACE_ID,
};

pub async fn entry<B>(jar: CookieJar, req: Request<B>, next: Next<B>) -> Response {
    let trace_id = random_string(6);
    let device_id = get_device_id_from_cookie(&jar);
    DEVICE_ID
        .scope(device_id, async {
            ACCOUNT
                .scope("".to_string(), async {
                    TRACE_ID
                        .scope(trace_id.clone(), async {
                            let mut resp = next.run(req).await;
                            let headers = resp.headers_mut();
                            set_no_cache_if_not_exist(headers);
                            // 忽略出错
                            let _ = set_header_if_not_exist(
                                headers,
                                "X-Trace-Id".to_string(),
                                trace_id,
                            );

                            resp
                        })
                        .await
                })
                .await
        })
        .await
}
