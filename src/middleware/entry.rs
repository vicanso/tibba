use crate::task_local::*;
use crate::util::{
    get_device_id_from_cookie, random_string, set_header_if_not_exist, set_no_cache_if_not_exist,
};
use axum::{body::Body, http::Request, middleware::Next, response::Response};
use axum_extra::extract::cookie::CookieJar;
use chrono::Utc;

pub async fn entry(jar: CookieJar, req: Request<Body>, next: Next) -> Response {
    let trace_id = random_string(6);
    let device_id = get_device_id_from_cookie(&jar);

    // 设置请求处理开始时间
    STARTED_AT
        .scope(Utc::now().timestamp_millis(), async {
            // 设置设备ID
            DEVICE_ID
                .scope(device_id, async {
                    // 设置账号
                    ACCOUNT
                        .scope("".to_string(), async {
                            // 设置请求的trace id
                            TRACE_ID
                                .scope(trace_id.clone(), async {
                                    let mut resp = next.run(req).await;
                                    let headers = resp.headers_mut();
                                    set_no_cache_if_not_exist(headers);
                                    // 忽略出错
                                    let _ =
                                        set_header_if_not_exist(headers, "X-Trace-Id", &trace_id);

                                    resp
                                })
                                .await
                        })
                        .await
                })
                .await
        })
        .await
}
