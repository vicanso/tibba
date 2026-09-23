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

//! SSRF 防护的端到端守卫：走真实的 reqwest 发送路径，而不是只测地址分类函数。
//!
//! 关键不变量是「域名校验发生在 reqwest 建连用的那次解析里」——只有把请求
//! 真正发出去才验得到。

use axum::Router;
use axum::http::Method;
use axum::routing::get;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tibba_request::{ClientBuilder, Error, Params};

/// 起一个本地服务端；本测试期望请求**根本到不了**它。
async fn spawn_server() -> SocketAddr {
    let app = Router::new().route("/", get(|| async { "reached" }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定本地端口");
    let addr = listener.local_addr().expect("取监听地址");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    addr
}

/// 退避基数。见 [`guarded_client`] 的说明。
const RETRY_BASE: Duration = Duration::from_secs(10);

fn guarded_client() -> tibba_request::Client {
    ClientBuilder::new("ssrf-test")
        .with_deny_internal_targets()
        // 故意开重试，且退避基数取得很大：拦截若被当成普通连接失败而重试，
        // 第一次退避就至少 5s（抖动区间 [base/2, base)），与正常的毫秒级返回
        // 拉开两个数量级——断言不受 CI 负载抖动影响
        .with_retry(3, RETRY_BASE)
        .build()
        .expect("构建客户端")
}

/// **核心守卫**：解析到内网的域名必须在建连时被拦下，并报 BlockedTarget。
///
/// `localhost` 解析到回环地址。此前域名校验是「自己先解析一次，再交给 reqwest
/// 解析第二次」，两次之间可以被 DNS rebinding 钻空子；现在校验就在 reqwest
/// 用来建连的解析器里。
#[tokio::test]
async fn hostname_resolving_to_loopback_is_blocked_at_connect() {
    let addr = spawn_server().await;
    let url = format!("http://localhost:{}/", addr.port());
    let started = Instant::now();
    let err = guarded_client()
        .request_raw(Params::new(Method::GET, &url))
        .await
        .expect_err("解析到回环地址的域名必须被拦截");

    assert!(
        matches!(err, Error::BlockedTarget { .. }),
        "应报 BlockedTarget，实际: {err}"
    );
    // 策略拒绝不能被重试：哪怕只重试一次也要等 RETRY_BASE/2 = 5s 以上
    assert!(
        started.elapsed() < RETRY_BASE / 2 - Duration::from_secs(2),
        "被拦截的请求不应触发重试，耗时 {:?}",
        started.elapsed()
    );
}

/// IP 字面量不经过解析器，由预检拦截。
#[tokio::test]
async fn loopback_ip_literal_is_blocked() {
    let addr = spawn_server().await;
    let err = guarded_client()
        .request_raw(Params::new(Method::GET, &format!("http://{addr}/")))
        .await
        .expect_err("回环 IP 字面量必须被拦截");
    assert!(matches!(err, Error::BlockedTarget { .. }), "{err}");
}

/// IPv6 字面量带方括号：此前 parse 失败落进 DNS 分支，报的是「解析失败」。
#[tokio::test]
async fn bracketed_ipv6_literal_reports_blocked_target() {
    let err = guarded_client()
        .request_raw(Params::new(Method::GET, "http://[::1]:9/"))
        .await
        .expect_err("IPv6 回环字面量必须被拦截");
    assert!(
        matches!(err, Error::BlockedTarget { .. }),
        "应报 BlockedTarget 而非 DNS 错误，实际: {err}"
    );
}

/// 云元数据端点的 NAT64 写法同样要拦——换个写法就绕过是 SSRF 的经典手法。
#[tokio::test]
async fn nat64_encoded_metadata_endpoint_is_blocked() {
    let err = guarded_client()
        .request_raw(Params::new(Method::GET, "http://[64:ff9b::a9fe:a9fe]/"))
        .await
        .expect_err("NAT64 夹带的元数据地址必须被拦截");
    assert!(matches!(err, Error::BlockedTarget { .. }), "{err}");
}

/// 未开启防护的客户端不受影响——解析器只在开关打开时挂载。
#[tokio::test]
async fn unguarded_client_still_reaches_localhost() {
    let addr = spawn_server().await;
    let body = ClientBuilder::new("plain")
        .build()
        .expect("构建客户端")
        .request_raw(Params::new(
            Method::GET,
            &format!("http://localhost:{}/", addr.port()),
        ))
        .await
        .expect("未开启防护时应能正常访问");
    assert_eq!(&body[..], b"reached");
}
