// Copyright 2026 Tree xie.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! 响应体上限的端到端守卫。
//!
//! 起一个真实的本地 HTTP 服务端来测，而不是直接调内部函数：这条限制的价值
//! 恰恰在于「读到越界的那一块就停」，只有走完整的 reqwest 读取路径才验得出来。

use axum::Router;
use axum::body::Body;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use std::net::SocketAddr;
use tibba_request::{Client, ClientBuilder};

/// 响应体大小，远超测试里设置的上限。
const BIG_BODY_LEN: usize = 256 * 1024;

/// 不带 Content-Length 的分块响应：模拟「对端不声明长度，边发边灌」。
async fn chunked_big() -> Response {
    let chunks = (0..64).map(|_| Ok::<_, std::io::Error>(vec![b'x'; BIG_BODY_LEN / 64]));
    Body::from_stream(futures::stream::iter(chunks)).into_response()
}

/// 带准确 Content-Length 的大响应。
async fn sized_big() -> Response {
    (
        [(header::CONTENT_TYPE, "application/octet-stream")],
        vec![b'x'; BIG_BODY_LEN],
    )
        .into_response()
}

/// 正常的小 JSON 响应。
async fn small_json() -> Response {
    ([(header::CONTENT_TYPE, "application/json")], r#"{"ok":true}"#).into_response()
}

/// 起一个本地服务端，返回其地址。
async fn spawn_server() -> SocketAddr {
    let app = Router::new()
        .route("/chunked", get(chunked_big))
        .route("/sized", get(sized_big))
        .route("/small", get(small_json));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定本地端口");
    let addr = listener.local_addr().expect("取监听地址");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

fn client(addr: SocketAddr, limit: Option<usize>) -> Client {
    let builder = ClientBuilder::new("test").with_base_url(format!("http://{addr}"));
    match limit {
        Some(limit) => builder.with_max_response_bytes(limit),
        None => builder.without_response_limit(),
    }
    .build()
    .expect("构建客户端")
}

/// 声明了 Content-Length 的超大响应：应在读取**之前**就被拒。
#[tokio::test]
async fn rejects_oversized_response_with_content_length() {
    let addr = spawn_server().await;
    let err = client(addr, Some(4096))
        .request_raw(tibba_request::Params::new(
            axum::http::Method::GET,
            "/sized",
        ))
        .await
        .expect_err("超限响应必须被拒绝");
    assert!(
        err.to_string().contains("response too large"),
        "应报超限，实际: {err}"
    );
}

/// 不声明长度的分块响应：必须在累计越界的那一块中止，而不是读完再判断。
#[tokio::test]
async fn rejects_oversized_chunked_response() {
    let addr = spawn_server().await;
    let err = client(addr, Some(4096))
        .request_raw(tibba_request::Params::new(
            axum::http::Method::GET,
            "/chunked",
        ))
        .await
        .expect_err("分块超限响应必须被拒绝");
    assert!(
        err.to_string().contains("response too large"),
        "应报超限，实际: {err}"
    );
}

/// 上限不得误伤正常响应。
#[tokio::test]
async fn normal_response_is_unaffected() {
    let addr = spawn_server().await;
    let body = client(addr, Some(4096))
        .request_raw(tibba_request::Params::new(
            axum::http::Method::GET,
            "/small",
        ))
        .await
        .expect("小响应应当正常返回");
    assert_eq!(&body[..], br#"{"ok":true}"#);
}

/// 显式关闭上限后，超大响应照常读完。
#[tokio::test]
async fn limit_can_be_disabled() {
    let addr = spawn_server().await;
    let body = client(addr, None)
        .request_raw(tibba_request::Params::new(
            axum::http::Method::GET,
            "/sized",
        ))
        .await
        .expect("关闭上限后应能读完");
    assert_eq!(body.len(), BIG_BODY_LEN);
}

/// 状态码本身仍按原有语义处理：默认上限足够大，不影响正常链路。
#[tokio::test]
async fn default_limit_allows_ordinary_payloads() {
    let addr = spawn_server().await;
    let client = ClientBuilder::new("test")
        .with_base_url(format!("http://{addr}"))
        .build()
        .expect("构建客户端");
    let body = client
        .request_raw(tibba_request::Params::new(
            axum::http::Method::GET,
            "/sized",
        ))
        .await
        .expect("256KiB 远小于默认 64MiB 上限");
    assert_eq!(body.len(), BIG_BODY_LEN);
}
