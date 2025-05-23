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
use tracing::{error, info};

type Result<T> = std::result::Result<T, Error>;

async fn run_http_detector(pool: &MySqlPool, detector: HttpDetector) -> Result<()> {
    let Ok(mut params) = HttpRequest::try_from(detector.url.as_str()) else {
        // add http stat
        return Ok(());
    };
    params.method = Some(detector.method);
    params.alpn_protocols = detector.alpn_protocols.unwrap_or_default();
    if let Some(headers) = detector.headers {
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
        params.ip_version = Some(detector.ip_version);
    }
    params.skip_verify = detector.skip_verify;
    let dns_servers = detector.dns_servers.unwrap_or_default();
    if !dns_servers.is_empty() {
        params.dns_servers = Some(dns_servers);
    }

    let do_request = async |params: HttpRequest| -> Result<()> {
        let stat = request(params).await;
        let mut status = 0;
        if stat.error.is_some()
            || stat.status.is_none()
            || stat.status.unwrap_or_default().as_u16() >= 400
        {
            status = 1;
        }
        let insert_params = HttpStatInsertParams {
            target_id: detector.id,
            target_name: detector.name,
            url: detector.url,
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
            status,
        };
        HttpStat::add_stat(pool, insert_params).await?;
        Ok(())
    };
    if let Some(resolves) = detector.resolves {
        for resolve in resolves {
            let Ok(ip) = resolve.parse::<IpAddr>() else {
                continue;
            };
            let mut new_params = params.clone();
            new_params.resolve = Some(ip);
            do_request.clone()(new_params).await?;
        }
    } else {
        do_request(params).await?;
    }
    Ok(())
}

async fn run_detector_stat() -> Result<(usize, usize)> {
    let pool = get_db_pool();
    let detectors = HttpDetector::list_enabled(pool).await?;
    let count = detectors.len();
    let mut success = 0;
    for detector in detectors {
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
                run_detector_stat().await;
                let job = Job::new_async("1/60 * * * * *", |uuid, mut l| {
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
                .map_err(|e| new_error(&e.to_string()))?;

                register_job_task("http_detector", job);
                Ok(())
            })
        }),
    );
}
