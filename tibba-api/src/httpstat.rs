// Copyright 2025 Tree xie.
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

use super::sql::get_db_pool;
use ctor::ctor;
use http::{HeaderMap, HeaderName, HeaderValue};
use http_stat::{HttpRequest, request};
use sqlx::MySqlPool;
use std::net::IpAddr;
use tibba_error::{Error, new_error};
use tibba_hook::register_before_task;
use tibba_model::{HttpDetector, HttpStat, HttpStatInsertParams};
use tibba_scheduler::{Job, register_job_task};
use time::OffsetDateTime;
use tracing::{error, info};

type Result<T> = std::result::Result<T, Error>;

async fn do_request(pool: &MySqlPool, detector: &HttpDetector, params: HttpRequest) -> Result<()> {
    let stat = request(params).await;
    let mut result = 0;
    if stat.error.is_some()
        || stat.status.is_none()
        || stat.status.unwrap_or_default().as_u16() >= 400
    {
        result = 1;
    }
    let insert_params = HttpStatInsertParams {
        target_id: detector.id,
        target_name: detector.name.clone(),
        url: detector.url.clone(),
        dns_lookup: stat.dns_lookup.map(|d| d.as_millis() as i32),
        quic_connect: stat.quic_connect.map(|d| d.as_millis() as i32),
        tcp_connect: stat.tcp_connect.map(|d| d.as_millis() as i32),
        tls_handshake: stat.tls_handshake.map(|d| d.as_millis() as i32),
        server_processing: stat.server_processing.map(|d| d.as_millis() as i32),
        content_transfer: stat.content_transfer.map(|d| d.as_millis() as i32),
        total: stat.total.map(|d| d.as_millis() as i32),
        addr: stat.addr.unwrap_or_default(),
        status_code: stat.status.map(|s| s.as_u16()),
        tls: stat.tls,
        alpn: stat.alpn,
        subject: stat.subject,
        issuer: stat.issuer,
        cert_not_before: stat.cert_not_before,
        cert_not_after: stat.cert_not_after,
        cert_cipher: stat.cert_cipher,
        cert_domains: stat.cert_domains.map(|d| d.join(",")),
        body_size: stat.body_size.map(|d| d as i32),
        error: stat.error,
        result,
    };
    HttpStat::add_stat(pool, insert_params).await?;
    Ok(())
}

async fn run_http_detector(pool: &MySqlPool, detector: HttpDetector) -> Result<()> {
    let Ok(mut params) = HttpRequest::try_from(detector.url.as_str()) else {
        HttpStat::add_stat(
            pool,
            HttpStatInsertParams {
                target_id: detector.id,
                target_name: detector.name.clone(),
                url: detector.url.clone(),
                result: 1,
                error: Some("url parse error".to_string()),
                ..Default::default()
            },
        )
        .await?;
        return Ok(());
    };
    params.method = Some(detector.method.clone());
    params.alpn_protocols = detector.alpn_protocols.clone().unwrap_or_default();
    if let Some(headers) = &detector.headers {
        let mut header_map = HeaderMap::new();
        for (key, value) in headers {
            let Ok(name) = HeaderName::try_from(key.as_str()) else {
                continue;
            };
            let Ok(value) = HeaderValue::try_from(value.as_str()) else {
                continue;
            };
            header_map.insert(name, value);
        }
        params.headers = Some(header_map);
    }
    if detector.ip_version > 0 {
        params.ip_version = Some(detector.ip_version as i32);
    }
    params.skip_verify = detector.skip_verify;
    let dns_servers = detector.dns_servers.clone().unwrap_or_default();
    if !dns_servers.is_empty() {
        params.dns_servers = Some(dns_servers);
    }
    let resolves = detector.resolves.clone().unwrap_or_default();
    if !resolves.is_empty() {
        for resolve in resolves {
            let Ok(ip) = resolve.parse::<IpAddr>() else {
                continue;
            };
            let mut new_params = params.clone();
            new_params.resolve = Some(ip);
            do_request(pool, &detector, new_params).await?;
        }
    } else {
        do_request(pool, &detector, params).await?;
    }
    Ok(())
}

async fn run_detector_stat() -> Result<(usize, usize)> {
    let pool = get_db_pool();
    let detectors = HttpDetector::list_enabled(pool).await?;
    let mut count = 0;
    let mut success = 0;
    let minutes = OffsetDateTime::now_utc().unix_timestamp() / 60;

    for detector in detectors {
        let interval = detector.interval.max(1);
        if minutes % (interval as i64) != 0 {
            continue;
        }
        count += 1;
        if let Err(e) = run_http_detector(pool, detector).await {
            error!(category = "http_detector", error = ?e, "run http detector failed");
        } else {
            success += 1;
        }
    }
    Ok((success, count))
}

#[ctor]
fn init() {
    register_before_task(
        "init_http_detector",
        u8::MAX,
        Box::new(|| {
            Box::pin(async {
                let job = Job::new_async("1/60 * * * * *", |_, _| {
                    Box::pin(async move {
                        match run_detector_stat().await {
                            Err(e) => {
                                error!(
                                    category = "http_detector",
                                    error = ?e,
                                    "run http detector failed"
                                );
                            }
                            Ok((success, count)) => {
                                info!(
                                    category = "http_detector",
                                    count, success, "run http detector success"
                                );
                            }
                        };
                    })
                })
                .map_err(new_error)?;
                register_job_task("http_detector", job);
                Ok(())
            })
        }),
    );
}
