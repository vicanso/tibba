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

use crate::config::{DivingConfig, must_get_diving_config};
use crate::sql::get_db_pool;
use ctor::ctor;
use http::Method;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use once_cell::sync::OnceCell;
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use std::sync::Arc;
use tibba_error::Error;
use tibba_hook::{BoxFuture, Task, register_task};
use tibba_llm::LlmCall;
use tibba_request::{Client, ClientBuilder, Params};
use tibba_scheduler::{Job, register_job_task};
use tracing::{error, info, warn};

type Result<T> = std::result::Result<T, Error>;

/// docker_analyses.status 枚举值
pub const STATUS_WAITING: i16 = 0;
pub const STATUS_PROCESSING: i16 = 1;
pub const STATUS_COMPLETED: i16 = 2;
pub const STATUS_FAILED: i16 = 3;

#[derive(Debug, FromRow)]
pub struct DockerAnalysisRecord {
    pub id: i64,
    pub user_id: i64,
    pub repo_name: String,
    pub tag: String,
    /// 推送方式：wecom / email / 空字符串
    pub notify_type: String,
    /// 推送目标：WeCom robot key 或收件邮箱地址
    pub notify_data: String,
}

/// 分析结果，同时保存 diving 原始诊断数据与 LLM 深度分析内容。
#[derive(Debug, Serialize)]
pub struct DockerAnalysisResult {
    /// diving 服务返回的原始 markdown 诊断数据
    pub diving_result: String,
    /// LLM 基于诊断数据生成的 markdown 分析报告
    pub llm_result: String,
    /// LLM 调用耗时（毫秒）
    pub elapsed_ms: u128,
    /// 是否与上次分析结论一致
    pub is_same_as_last: bool,
}

pub struct DockerAnalysisModel;

impl DockerAnalysisModel {
    /// 查询相同 user_id + repo_name + tag 且处于等待或处理中的记录 id，不存在返回 None。
    pub async fn find_pending_id(
        pool: &PgPool,
        user_id: i64,
        repo_name: &str,
        tag: &str,
    ) -> Result<Option<i64>> {
        let row: Option<(i64,)> = sqlx::query_as(
            r#"SELECT id FROM docker_analyses
               WHERE user_id = $1 AND repo_name = $2 AND tag = $3 AND status = ANY($4)
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(repo_name)
        .bind(tag)
        .bind(&[STATUS_WAITING, STATUS_PROCESSING][..])
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::new(e.to_string()).with_category("docker"))?;
        Ok(row.map(|r| r.0))
    }

    /// 插入一条初始状态为等待处理的分析记录，返回新记录 id。
    pub async fn insert(
        pool: &PgPool,
        user_id: i64,
        repo_name: &str,
        tag: &str,
        notify_type: &str,
        notify_data: &str,
    ) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            r#"INSERT INTO docker_analyses (user_id, repo_name, tag, notify_type, notify_data)
               VALUES ($1, $2, $3, $4, $5) RETURNING id"#,
        )
        .bind(user_id)
        .bind(repo_name)
        .bind(tag)
        .bind(notify_type)
        .bind(notify_data)
        .fetch_one(pool)
        .await
        .map_err(|e| Error::new(e.to_string()).with_category("docker"))?;
        Ok(row.0)
    }

    /// 查询24小时内处于 STATUS_WAITING 的记录 id 列表。
    pub async fn list_waiting_ids(pool: &PgPool) -> Result<Vec<i64>> {
        let rows: Vec<(i64,)> = sqlx::query_as(
            r#"SELECT id FROM docker_analyses
               WHERE status = $1
                 AND created >= NOW() - INTERVAL '24 hours'
               ORDER BY id"#,
        )
        .bind(STATUS_WAITING)
        .fetch_all(pool)
        .await
        .map_err(|e| Error::new(e.to_string()).with_category("docker"))?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// 尝试将单条记录从 STATUS_WAITING 原子性地标记为 STATUS_PROCESSING。
    /// 返回记录详情表示抢占成功，None 表示已被其他实例抢先处理。
    pub async fn try_mark_processing(
        pool: &PgPool,
        id: i64,
    ) -> Result<Option<DockerAnalysisRecord>> {
        let record = sqlx::query_as::<_, DockerAnalysisRecord>(
            r#"UPDATE docker_analyses
               SET status = $1, modified = NOW()
               WHERE id = $2 AND status = $3
               RETURNING id, user_id, repo_name, tag, notify_type, notify_data"#,
        )
        .bind(STATUS_PROCESSING)
        .bind(id)
        .bind(STATUS_WAITING)
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::new(e.to_string()).with_category("docker"))?;
        Ok(record)
    }

    /// 将指定记录标记为 STATUS_COMPLETED，并写入分析结果（JSON）。
    pub async fn mark_completed(pool: &PgPool, id: i64, result: &str) -> Result<()> {
        sqlx::query(
            r#"UPDATE docker_analyses
               SET status = $1, result = $2, modified = NOW()
               WHERE id = $3"#,
        )
        .bind(STATUS_COMPLETED)
        .bind(result)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| Error::new(e.to_string()).with_category("docker"))?;
        Ok(())
    }

    /// 查询同一 repo_name + tag 最近一次成功分析的 llm_result，排除当前记录 id。
    /// 返回 None 表示没有历史记录或解析失败（不阻断主流程）。
    pub async fn find_last_llm_result(
        pool: &PgPool,
        repo_name: &str,
        tag: &str,
        exclude_id: i64,
    ) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            r#"SELECT result FROM docker_analyses
               WHERE repo_name = $1 AND tag = $2 AND status = $3 AND id != $4
               ORDER BY id DESC
               LIMIT 1"#,
        )
        .bind(repo_name)
        .bind(tag)
        .bind(STATUS_COMPLETED)
        .bind(exclude_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::new(e.to_string()).with_category("docker"))?;

        let llm_result = row
            .and_then(|r| r.0)
            .and_then(|json_str| serde_json::from_str::<serde_json::Value>(&json_str).ok())
            .and_then(|v| {
                v.get("llm_result")
                    .and_then(|s| s.as_str())
                    .map(String::from)
            });
        Ok(llm_result)
    }

    /// 将指定记录标记为 STATUS_FAILED，并写入错误信息。
    pub async fn mark_failed(pool: &PgPool, id: i64, reason: &str) -> Result<()> {
        sqlx::query(
            r#"UPDATE docker_analyses
               SET status = $1, result = $2, modified = NOW()
               WHERE id = $3"#,
        )
        .bind(STATUS_FAILED)
        .bind(reason)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| Error::new(e.to_string()).with_category("docker"))?;
        Ok(())
    }
}

async fn run_docker_analysis() -> Result<usize> {
    let pool = get_db_pool();
    let ids = DockerAnalysisModel::list_waiting_ids(pool).await?;

    if ids.is_empty() {
        return Ok(0);
    }

    let mut completed = 0usize;
    for id in ids {
        // 多实例竞争：每次处理前尝试原子抢占，失败则跳过
        let Some(record) = DockerAnalysisModel::try_mark_processing(pool, id).await? else {
            continue;
        };

        match analyze_image(&record).await {
            Ok(result) => {
                let json = serde_json::to_string(&result).unwrap_or_else(|_| String::from("{}"));
                if let Err(e) = DockerAnalysisModel::mark_completed(pool, id, &json).await {
                    error!(id, error = %e, "mark docker analysis completed failed");
                } else {
                    completed += 1;
                    if !result.is_same_as_last {
                        notify_result(&record, &result).await;
                    }
                }
            }
            Err(e) => {
                error!(id, error = %e, "docker image analysis failed");
                let _ = DockerAnalysisModel::mark_failed(pool, id, &e.to_string()).await;
            }
        }
    }

    Ok(completed)
}

static DIVING_CLIENT: OnceCell<Client> = OnceCell::new();

fn get_diving_client() -> Result<&'static Client> {
    DIVING_CLIENT.get_or_try_init(|| {
        ClientBuilder::new("diving")
            .with_base_url(must_get_diving_config().url.clone())
            .build()
            .map_err(|e| Error::new(e.to_string()).with_category("docker"))
    })
}

const ANALYSIS_SYSTEM_PROMPT: &str = r#"
你现在是一位极其务实、精通 Docker 底层架构（特别是 OverlayFS 分层文件系统）的 DevSecOps 资深专家。

我将为你提供 Docker 镜像的深度分析数据。请严格遵循“异常驱动”原则进行诊断。

【⚠️ 绝对执行的分析铁律 (CRITICAL RULES)】
1. 极度静默：表现良好的指标（如浪费率为 0%、非 root 用户运行、无真实密钥泄露）绝对禁止提及。不写任何“未发现问题”、“表现优秀”的废话。
2. OverlayFS 穿透判定（防伪优化）：
   - 在处理 `[Contains Package Manager Cache]` 等冗余空间问题时，必须穿透分析自定义层（Layers）。
   - 如果自定义层中没有执行过包管理器安装（如仅有 COPY 指令），说明缓存 100% 来自基础镜像。此时**绝对禁止**建议追加跨层的 `RUN rm -rf` 命令！
   - 针对基础镜像的冗余，唯一合规的建议是：“更换为 slim/alpine/distroless/scratch 等精简基底”，或者明确标注“这是可接受的基础镜像原生冗余，无需处理”。
3. 权限对齐检查：严查 `COPY` 或 `ADD` 进来的文件所有权（Owner）是否与运行时用户（User）存在倒挂风险。

请严格按以下精简格式输出报告（如果未发现需要工程师动手修改的真实问题，请直接回复：“🟢 镜像健康，无需优化”）：

### 🚨 核心异常与真实痛点
- [精确指出异常指标及其产生的根源。一句废话不要有]

### 🛠️ 必须执行的重构代码
- [给出优化后的 Dockerfile 片段，只写需要改动或替换的那几行]

---"#;

/// 调用 diving 服务获取镜像诊断数据（markdown 格式）。
async fn fetch_diving_result(record: &DockerAnalysisRecord) -> Result<String> {
    let client = get_diving_client()?;
    let image = format!("{}:{}", record.repo_name, record.tag);
    let query = [
        ("image", image.as_str()),
        ("format", "markdown"),
        ("skipBase", "true"),
    ];
    let bytes = client
        .request_raw(Params {
            method: Method::GET,
            url: "/api/analyze",
            query: Some(query.as_slice()),
            body: None::<&[(&str, &str)]>,
            timeout: None,
        })
        .await
        .map_err(|e| Error::new(e.to_string()).with_category("docker"))?;
    String::from_utf8(bytes.to_vec()).map_err(|e| Error::new(e.to_string()).with_category("docker"))
}

/// 调用 diving 获取诊断数据后，交给 LLM 进行深度分析，返回结构化结果。
async fn analyze_image(record: &DockerAnalysisRecord) -> Result<DockerAnalysisResult> {
    let diving_result = fetch_diving_result(record).await?;

    info!(id = record.id, "diving image success");

    let pool = get_db_pool();
    let prev_llm =
        DockerAnalysisModel::find_last_llm_result(pool, &record.repo_name, &record.tag, record.id)
            .await
            .unwrap_or(None);

    // 借用 prev_llm 构建 user_message，保留所有权供后续判断使用
    let user_message = if let Some(ref prev) = prev_llm {
        format!(
            "# 本次镜像诊断数据\n\n\
             {diving_result}\n\n\
             ---\n\n\
             # 上一次分析结论（供对比）\n\n\
             {prev}\n\n\
             请将本次诊断数据与上一次结论进行对比。若两次结论基本一致，直接输出：\
             **与上次分析结论一致，无需调整。**"
        )
    } else {
        format!("本次镜像诊断数据：\n\n{diving_result}")
    };

    let config = must_get_diving_config();
    let llm_start = std::time::Instant::now();
    let resp = LlmCall::new(&config.llm_api_key, &config.llm_model, &user_message)
        .with_base_url(&config.llm_url)
        .with_system_message(ANALYSIS_SYSTEM_PROMPT)
        .chat()
        .await
        .map_err(|e| Error::new(e.to_string()).with_category("docker"))?;
    let elapsed_ms = llm_start.elapsed().as_millis();

    info!(
        id = record.id,
        model = resp.model,
        input_tokens = resp.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
        output_tokens = resp.usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
        elapsed_ms,
        "docker image llm analysis done",
    );
    if resp.content.is_empty() {
        warn!(id = record.id, "llm returned empty content");
    }

    // LLM 返回"与上次一致"且确实有历史结论时，直接复用上次的 llm_result
    let is_same = resp.content.contains("与上次分析结论一致");
    let (llm_result, is_same_as_last) = match (is_same, prev_llm) {
        (true, Some(prev)) => (prev, true),
        _ => (resp.content, false),
    };

    info!(id = record.id, is_same_as_last, "llm analysis success");

    Ok(DockerAnalysisResult {
        diving_result,
        llm_result,
        elapsed_ms,
        is_same_as_last,
    })
}

async fn send_wecom_notification(
    token: &str,
    record: &DockerAnalysisRecord,
    result: &DockerAnalysisResult,
) -> Result<()> {
    let content = format!(
        "**Docker Image Analysis Completed**\n\
         - Image: `{}:{}`\n\
         - Analysis ID: {}\n\
         - Elapsed: {}ms\n\n\
         {}",
        record.repo_name, record.tag, record.id, result.elapsed_ms, result.llm_result
    );
    let url = format!(
        "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key={}",
        token
    );
    let body = serde_json::json!({
        "msgtype": "markdown",
        "markdown": { "content": content }
    });
    reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| Error::new(e.to_string()).with_category("docker"))?;
    Ok(())
}

async fn send_email_notification(
    config: &DivingConfig,
    to: &str,
    record: &DockerAnalysisRecord,
    result: &DockerAnalysisResult,
) -> Result<()> {
    let smtp_host = config
        .smtp_host
        .as_deref()
        .ok_or_else(|| Error::new("smtp_host not configured").with_category("docker"))?;
    let from_addr = config.smtp_from.as_deref().unwrap_or("noreply@tibba.io");

    let subject = format!("Docker Analysis: {}:{}", record.repo_name, record.tag);
    let body = format!(
        "Image: {}:{}\nAnalysis ID: {}\nElapsed: {}ms\n\n{}\n\n---\nDiving Result:\n{}",
        record.repo_name,
        record.tag,
        record.id,
        result.elapsed_ms,
        result.llm_result,
        result.diving_result,
    );

    let email = Message::builder()
        .from(
            from_addr
                .parse()
                .map_err(|e: lettre::address::AddressError| {
                    Error::new(e.to_string()).with_category("docker")
                })?,
        )
        .to(to.parse().map_err(|e: lettre::address::AddressError| {
            Error::new(e.to_string()).with_category("docker")
        })?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .map_err(|e| Error::new(e.to_string()).with_category("docker"))?;

    let mut builder = AsyncSmtpTransport::<Tokio1Executor>::relay(smtp_host)
        .map_err(|e| Error::new(e.to_string()).with_category("docker"))?;

    if let Some(port) = config.smtp_port {
        builder = builder.port(port);
    }
    if let (Some(username), Some(password)) = (&config.smtp_username, &config.smtp_password) {
        builder = builder.credentials(Credentials::new(username.clone(), password.clone()));
    }

    builder
        .build()
        .send(email)
        .await
        .map_err(|e| Error::new(e.to_string()).with_category("docker"))?;
    Ok(())
}

async fn notify_result(record: &DockerAnalysisRecord, result: &DockerAnalysisResult) {
    let config = must_get_diving_config();

    // 优先使用记录中存储的推送方式
    if !record.notify_type.is_empty() && !record.notify_data.is_empty() {
        match record.notify_type.as_str() {
            "wecom" => {
                if let Err(e) = send_wecom_notification(&record.notify_data, record, result).await {
                    error!(id = record.id, error = %e, "send wecom notification failed");
                }
            }
            "email" => {
                if let Err(e) =
                    send_email_notification(config, &record.notify_data, record, result).await
                {
                    error!(id = record.id, error = %e, "send email notification failed");
                }
            }
            other => {
                warn!(
                    id = record.id,
                    notify_type = other,
                    "unknown notify_type, skipped"
                );
            }
        }
        return;
    }

    // 回退到全局配置
    if let Some(token) = &config.notify_wecom {
        if let Err(e) = send_wecom_notification(token, record, result).await {
            error!(id = record.id, error = %e, "send wecom notification failed");
        }
    }
    if let Some(email) = &config.notify_email {
        if let Err(e) = send_email_notification(config, email, record, result).await {
            error!(id = record.id, error = %e, "send email notification failed");
        }
    }
}

struct DockerAnalysisTask;

impl Task for DockerAnalysisTask {
    fn before(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move {
            // 每分钟执行一次
            let job = Job::new_async("0 * * * * *", |_, _| {
                let category = "docker_analysis";
                Box::pin(async move {
                    match run_docker_analysis().await {
                        Err(e) => {
                            error!(category, error = %e, "run docker analysis failed");
                        }
                        Ok(completed) => {
                            info!(category, completed, "run docker analysis success");
                        }
                    }
                })
            })
            .map_err(Error::new)?;
            register_job_task("docker_analysis", job);
            Ok(true)
        })
    }
}

#[ctor(unsafe)]
fn init() {
    register_task("docker_analysis", Arc::new(DockerAnalysisTask));
}
