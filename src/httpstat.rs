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
use chrono::DateTime;
use ctor::ctor;
use futures::future::join_all;
use http::Uri;
use http::{HeaderMap, HeaderName, HeaderValue};
use http_stat::{HttpRequest, request};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::{net::IpAddr, time::Duration};
use tibba_error::{Error, new_error};
use tibba_hook::register_before_task;
use tibba_model::{
    AlarmConfig, Configuration, HttpDetector, HttpStat, HttpStatInsertParams, Model, ResultValue,
};
use tibba_scheduler::{Job, register_job_task};
use time::OffsetDateTime;
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tracing::{error, info};

type Result<T> = std::result::Result<T, Error>;

#[derive(Serialize, Deserialize, Debug)]
struct JsResponse {
    status: u16,
    body: String,
    headers: HashMap<String, String>,
}

fn run_js_detect(resp: JsResponse, detect_script: &str) -> Result<()> {
    if detect_script.is_empty() {
        return Ok(());
    }
    let ctx = quick_js::Context::new().map_err(new_error)?;
    let content = serde_json::to_string(&resp).map_err(new_error)?;
    let mut script = r#"
(function(response) {
    try {
        response.body = JSON.parse(response.body);
    } finally {
    }
    __script__ 
})(__response__);
"#
    .to_string();
    script = script.replace("__response__", &content);
    script = script.replace("__script__", detect_script);
    ctx.eval(&script).map_err(new_error)?;
    Ok(())
}

async fn do_request(
    pool: &MySqlPool,
    detector: &HttpDetector,
    mut params: HttpRequest,
) -> Result<()> {
    if detector.random_querystring {
        let id = nanoid::nanoid!(8);
        let uri = params.uri.to_string();
        let uri = if uri.contains('?') {
            format!("{}&{}", uri, id)
        } else {
            format!("{}?{}", uri, id)
        };
        if let Ok(uri) = Uri::try_from(uri) {
            params.uri = uri;
        }
    }
    let url = params.uri.to_string();
    let stat = timeout(Duration::from_secs(60), request(params))
        .await
        .unwrap_or_else(|e| http_stat::HttpStat {
            error: Some(e.to_string()),
            ..Default::default()
        });
    let mut result = ResultValue::Success;
    let mut err = stat.error;

    if err.is_some() || stat.status.is_none() || stat.status.unwrap_or_default().as_u16() >= 400 {
        result = ResultValue::Failed;
        if err.is_none() {
            err = Some(format!(
                "http status code is >= 400, status: {}",
                stat.status.unwrap_or_default().as_u16()
            ));
        }
    }
    if let Some(cert_not_after) = &stat.cert_not_after {
        if let Ok(cert_not_after) = DateTime::parse_from_str(cert_not_after, "%Y-%m-%d %H:%M:%S %z")
        {
            // 提前7天设置为失败
            if cert_not_after.timestamp() < chrono::Utc::now().timestamp() - 7 * 24 * 3600 {
                err = Some("certificate will expired in 7 days".to_string());
                result = ResultValue::Failed;
            }
        }
    }
    if result == ResultValue::Success && detector.script.is_some() {
        if let Err(e) = run_js_detect(
            JsResponse {
                status: stat.status.unwrap_or_default().as_u16(),
                body: String::from_utf8(stat.body.unwrap_or_default().to_vec()).unwrap_or_default(),
                headers: stat
                    .headers
                    .unwrap_or_default()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string()))
                    .collect(),
            },
            &detector.script.clone().unwrap_or_default(),
        ) {
            result = ResultValue::Failed;
            err = Some(e.to_string());
        }
    }

    let insert_params = HttpStatInsertParams {
        target_id: detector.id,
        target_name: detector.name.clone(),
        url,
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
        error: err,
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
    markdown_v2: WeComMarkDown,
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
    let success = Arc::new(AtomicI32::new(0));
    let minutes = OffsetDateTime::now_utc().unix_timestamp() / 60;

    // 最大并行任务数
    let max_concurrent = 3;
    let semaphore = Arc::new(Semaphore::new(max_concurrent));
    let mut handles = vec![];

    for detector in detectors {
        let interval = detector.interval.max(1);
        if minutes % (interval as i64) != 0 {
            continue;
        }
        count += 1;
        let permit = Arc::clone(&semaphore);
        let success = success.clone();
        let handle = tokio::spawn(async move {
            //  获取许可
            let Ok(_permit) = permit.acquire().await else {
                return;
            };
            if let Err(e) = run_http_detector(pool, detector).await {
                error!(category = "http_detector", error = ?e, "run http detector failed");
            } else {
                success.fetch_add(1, Ordering::Relaxed);
            }
            // 任务结束自动释放许可
        });
        handles.push(handle);
    }
    let results = join_all(handles).await;
    for result in results {
        if let Err(e) = result {
            error!(category = "http_detector", error = ?e, "join all handles failed");
        }
    }

    Ok((success.load(Ordering::Relaxed), count))
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct FailedTarget {
    target_id: u64,
    alarm_time: i64,
    alarm_count: i32,
}

#[derive(Serialize, Deserialize)]
struct StatAlarmCache {
    last_check_time: i64,
    failed_targets: Vec<FailedTarget>,
}

#[derive(Default, Debug)]
struct StatAlarmParam {
    stat: HttpStat,
    verbose: bool,
    alarm_config: Option<AlarmConfig>,
}

impl StatAlarmParam {
    fn to_wecom_markdown(&self) -> String {
        let mut arr = vec![];
        let state = &self.stat;
        let is_failed = state.result == ResultValue::Failed as u8;
        if is_failed {
            arr.push(format!("# {}：检测失败", state.target_name));
            arr.push(format!("*出错原因: {}*", state.error));
        } else {
            arr.push(format!("# {}：恢复正常", state.target_name));
        }
        if self.verbose && is_failed {
            if !state.addr.is_empty() {
                arr.push(format!("- 地址: {}", state.addr));
            }
            if state.dns_lookup > 0 {
                arr.push(format!("- DNS解析时间: {}ms", state.dns_lookup));
            }
            if state.quic_connect > 0 {
                arr.push(format!("- QUIC连接时间: {}ms", state.quic_connect));
            }
            if state.tcp_connect > 0 {
                arr.push(format!("- TCP连接时间: {}ms", state.tcp_connect));
            }
            if state.tls_handshake > 0 {
                arr.push(format!("- TLS握手时间: {}ms", state.tls_handshake));
            }
            if state.server_processing > 0 {
                arr.push(format!("- 服务器处理时间: {}ms", state.server_processing));
            }
            if state.content_transfer > 0 {
                arr.push(format!("- 内容传输时间: {}ms", state.content_transfer));
            }
            if state.total > 0 {
                arr.push(format!("- 总耗时: {}ms", state.total));
            }
        }
        arr.join("\n")
    }
}

async fn send_alarms(alarm_params: Vec<StatAlarmParam>, alarm_config: AlarmConfig) -> u32 {
    // 先发送指定了url的告警
    let mut success = 0;
    let send_markdown = async |content: String, url: String| -> Result<()> {
        match reqwest::Client::new()
            .post(&url)
            .timeout(Duration::from_secs(10))
            .json(&WeComMarkDownMessage {
                msgtype: "markdown_v2".to_string(),
                markdown_v2: WeComMarkDown { content },
            })
            .send()
            .await
        {
            Ok(res) => {
                if res.status().is_success() {
                    Ok(())
                } else {
                    Err(new_error(format!(
                        "send alarm message failed, status: {}",
                        res.status()
                    )))
                }
            }
            Err(e) => Err(new_error(e.to_string())),
        }?;
        Ok(())
    };
    let category = "http_stat_alarm";
    let mut contents = vec![];
    for param in alarm_params.iter() {
        if let Some(alarm_config) = &param.alarm_config {
            if let Err(e) = send_markdown(param.to_wecom_markdown(), alarm_config.url.clone()).await
            {
                error!(category, error = ?e, "send alarm message failed");
            } else {
                success += 1;
                info!(
                    category,
                    service = param.stat.target_name,
                    "send alarm message success"
                );
            }
            continue;
        }
        contents.push(param.to_wecom_markdown());
    }
    if !contents.is_empty() && !alarm_config.url.is_empty() {
        if let Err(e) = send_markdown(contents.join("\n"), alarm_config.url).await {
            error!(category, error = ?e, "send alarm message failed");
        } else {
            success += 1;
            info!(category, "send alarm message success");
        }
    }
    success
}

async fn run_stat_alarm() -> Result<(i32, i32)> {
    let task = "http_alarm_task";
    let locked = get_redis_cache()
        .lock(task, Some(Duration::from_secs(120)))
        .await?;
    if !locked {
        return Ok((0, -1));
    }
    let pool = get_db_pool();
    let alarm_config =
        if let Ok(Some(alarm_config)) = Configuration::get_alarm_config(pool, "httpstat").await {
            alarm_config
        } else {
            let robot_url = env::var("WECOM_ROBOT").unwrap_or_default();
            AlarmConfig {
                category: "httpstat".to_string(),
                url: robot_url.to_string(),
            }
        };

    let key = "http_alarm_targets_cache";
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

    let mut trigger_targets = vec![];
    let mut alarm_params = vec![];
    let mut failed_targets = vec![];
    for (target_id, stat) in stat_map.iter() {
        let is_failed = stat.result == ResultValue::Failed as u8;
        if is_failed {
            if let Some(fail_target) = alarm_cache
                .failed_targets
                .iter()
                .find(|t| t.target_id == *target_id)
            {
                failed_targets.push(fail_target.clone());
            } else {
                failed_targets.push(FailedTarget {
                    target_id: *target_id,
                    ..Default::default()
                });
            }
        }
        // 如果成功，而且原有失败列表中没有，则跳过
        if !is_failed
            && !alarm_cache
                .failed_targets
                .iter()
                .any(|t| t.target_id == *target_id)
        {
            continue;
        }

        let mut status_changed = true;

        // 如果失败，而且失败记录发送时间在1小时内，则跳过
        if is_failed {
            if let Some(fail_target) = failed_targets
                .iter_mut()
                .find(|t| t.target_id == *target_id)
            {
                // 如果状态失败，而且在失败记录中存在，则状态未改变
                if fail_target.alarm_count > 0 {
                    status_changed = false;
                }
                let offset = fail_target.alarm_count.min(12) as i64 * 2 * 3600;
                if fail_target.alarm_time > now - offset {
                    continue;
                }
                // 如果不是，则记录时间，并增加计数
                fail_target.alarm_time = now;
                fail_target.alarm_count += 1;
            }
        }

        let Ok(Some(detector)) = HttpDetector::get_by_id(pool, *target_id).await else {
            continue;
        };
        if trigger_targets.contains(&*target_id) {
            continue;
        }
        trigger_targets.push(*target_id);

        // 如果仅在状态变更时推送告警，而且状态未改变，则跳过
        if detector.alarm_on_change && !status_changed {
            continue;
        }
        if !detector.alarm_url.is_empty() {
            alarm_params.push(StatAlarmParam {
                stat: stat.clone(),
                verbose: detector.verbose,
                alarm_config: Some(AlarmConfig {
                    category: "httpstat".to_string(),
                    url: detector.alarm_url,
                }),
            });
            continue;
        }
        alarm_params.push(StatAlarmParam {
            stat: stat.clone(),
            verbose: detector.verbose,
            ..Default::default()
        });
    }
    let failed = failed_targets.len() as i32;
    if !alarm_params.is_empty() {
        let success = send_alarms(alarm_params, alarm_config).await;
        info!(category = task, success, "send alarm message done");
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
                let job = Job::new_async("0 * * * * *", |_, _| {
                    let category = "http_detector";
                    Box::pin(async move {
                        if let Ok(delay) = humantime::parse_duration(
                            std::env::var("HTTP_DETECTOR_TASK_DELAY")
                                .unwrap_or_default()
                                .as_str(),
                        ) {
                            tokio::time::sleep(delay).await;
                        }
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
                let job = Job::new_async("30 */5 * * * *", |_, _| {
                    let category = "http_stat_alarm";
                    Box::pin(async move {
                        if let Ok(delay) = humantime::parse_duration(
                            std::env::var("HTTP_DETECTOR_TASK_DELAY")
                                .unwrap_or_default()
                                .as_str(),
                        ) {
                            tokio::time::sleep(delay).await;
                        }
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
