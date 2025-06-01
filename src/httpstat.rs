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
use crate::cache::get_redis_cache;
use ctor::ctor;
use http::{HeaderMap, HeaderName, HeaderValue};
use http_stat::{HttpRequest, request};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use std::collections::HashMap;
use std::env;
use std::{net::IpAddr, time::Duration};
use tibba_error::{Error, new_error};
use tibba_hook::register_before_task;
use tibba_model::{HttpDetector, HttpStat, HttpStatInsertParams, ResultValue};
use tibba_scheduler::{Job, register_job_task};
use time::OffsetDateTime;
use tracing::{error, info};

type Result<T> = std::result::Result<T, Error>;

async fn do_request(pool: &MySqlPool, detector: &HttpDetector, params: HttpRequest) -> Result<()> {
    let stat = request(params).await;
    let mut result = ResultValue::Success;
    if stat.error.is_some()
        || stat.status.is_none()
        || stat.status.unwrap_or_default().as_u16() >= 400
    {
        result = ResultValue::Failed;
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
        alpn: Some(stat.alpn.unwrap_or(http_stat::ALPN_HTTP1.to_string())),
        subject: stat.subject,
        issuer: stat.issuer,
        cert_not_before: stat.cert_not_before,
        cert_not_after: stat.cert_not_after,
        cert_cipher: stat.cert_cipher,
        cert_domains: stat.cert_domains.map(|d| d.join(",")),
        body_size: stat.body_size.map(|d| d as i32),
        error: stat.error,
        result: result as u8,
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
                result: ResultValue::Failed as u8,
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

#[derive(Serialize)]
struct WeComMarkDown {
    content: String,
}

#[derive(Serialize)]
struct WeComMarkDownMessage {
    msgtype: String,
    markdown: WeComMarkDown,
}

async fn run_detector_stat() -> Result<(i32, i32)> {
    let locked = get_redis_cache()
        .lock("http_detector_task", Some(Duration::from_secs(30)))
        .await?;
    if !locked {
        return Ok((0, -1));
    }
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

#[derive(Serialize, Deserialize)]
struct StatAlarmCache {
    last_check_time: i64,
    failed_targets: Vec<u64>,
}

async fn run_stat_alarm() -> Result<(i32, i32)> {
    let task = "http_alarm_task";
    let locked = get_redis_cache()
        .lock(task, Some(Duration::from_secs(90)))
        .await?;
    if !locked {
        return Ok((0, -1));
    }
    let robot_url = env::var("WECOM_ROBOT").unwrap_or_default();
    if robot_url.is_empty() {
        return Ok((0, -1));
    }
    let key = "http_alarm_cache";
    let mut alarm_cache = StatAlarmCache {
        last_check_time: chrono::Utc::now().timestamp() - 5 * 60,
        failed_targets: vec![],
    };
    if let Ok(Some(result)) = get_redis_cache().get_struct::<StatAlarmCache>(key).await {
        alarm_cache.last_check_time = result.last_check_time;
        alarm_cache.failed_targets = result.failed_targets;
    };
    // 每次只查询10秒前的数据
    let now = chrono::Utc::now().timestamp() - 10;

    let pool = get_db_pool();
    let last_check_time = chrono::DateTime::from_timestamp(alarm_cache.last_check_time, 0)
        .ok_or(new_error("parse time error"))?
        .to_rfc3339();
    let now_check_time = chrono::DateTime::from_timestamp(now, 0)
        .ok_or(new_error("parse time error"))?
        .to_rfc3339();
    let stats = HttpStat::list_by_modified(pool, (&last_check_time, &now_check_time)).await?;

    let mut content = vec![];
    let mut failed_targets = vec![];

    // 因为相同的target id有可能会有多个http stat
    // 因此需要target id去重，若有失败的优先使用
    let mut stat_map: HashMap<u64, HttpStat> = HashMap::new();
    for stat in stats {
        let is_failed = stat.result == ResultValue::Failed as u8;
        let target_id = stat.target_id;
        if let Some(value) = stat_map.get_mut(&target_id) {
            if is_failed {
                *value = stat;
                continue;
            }
        }
        stat_map.insert(target_id, stat);
    }

    let count = stat_map.len() as i32;

    for (target_id, stat) in stat_map.iter() {
        let is_failed = stat.result == ResultValue::Failed as u8;
        if is_failed && !failed_targets.contains(target_id) {
            failed_targets.push(*target_id);
        }
        // 如果成功，而且非失败列表，则跳过
        if !is_failed && !alarm_cache.failed_targets.contains(target_id) {
            continue;
        }

        // 如果失败，而且在失败记录，则不需要重复发送
        if is_failed && alarm_cache.failed_targets.contains(target_id) {
            continue;
        }

        let status = if stat.result == ResultValue::Success as u8 {
            r#"<font color="info">成功</font>"#
        } else {
            r#"<font color="warning">失败</font>"#
        };
        let msg = format!(">{}: {}", stat.target_name, status);
        if !content.contains(&msg) {
            content.push(msg);
        }
    }
    let failed = failed_targets.len() as i32;
    if !content.is_empty() {
        match reqwest::Client::new()
            .post(&robot_url)
            .timeout(Duration::from_secs(10))
            .json(&WeComMarkDownMessage {
                msgtype: "markdown".to_string(),
                markdown: WeComMarkDown {
                    content: content.join("\n"),
                },
            })
            .send()
            .await
        {
            Ok(res) => {
                if res.status().is_success() {
                    info!(category = task, "send alarm message success");
                } else {
                    error!(category = task, status = ?res.status(), "send alarm message failed");
                }
            }
            Err(e) => {
                error!(category = task, error = ?e, "send alarm message failed");
            }
        }
    }

    if let Err(e) = get_redis_cache()
        .set_struct(
            key,
            &StatAlarmCache {
                last_check_time: now + 1,
                failed_targets,
            },
            Some(Duration::from_secs(3600)),
        )
        .await
    {
        error!(category = task, error = ?e, "set last check time failed");
    }
    Ok((failed, count))
}

#[ctor]
fn init() {
    register_before_task(
        "init_http_detector",
        u8::MAX,
        Box::new(|| {
            Box::pin(async {
                // 每分钟
                let job = Job::new_async("every 60 seconds", |_, _| {
                    let category = "http_detector";
                    Box::pin(async move {
                        match run_detector_stat().await {
                            Err(e) => {
                                error!(
                                    category,
                                    error = ?e,
                                    "run http detector failed"
                                );
                            }
                            Ok((success, count)) => {
                                if count >= 0 {
                                    info!(category, count, success, "run http detector success");
                                }
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

    register_before_task(
        "init_stat_alarm",
        u8::MAX,
        Box::new(|| {
            Box::pin(async {
                // 每5分钟
                let job = Job::new_async("every 5 minutes", |_, _| {
                    let category = "http_stat_alarm";
                    Box::pin(async move {
                        match run_stat_alarm().await {
                            Err(e) => {
                                error!(
                                    category,
                                    error = ?e,
                                    "run http stat alarm failed"
                                );
                            }
                            Ok((failed, count)) => {
                                if count >= 0 {
                                    info!(category, failed, count, "run http stat alarm success");
                                }
                            }
                        }
                    })
                })
                .map_err(new_error)?;
                register_job_task("http_stat_alarm", job);
                Ok(())
            })
        }),
    );
}
