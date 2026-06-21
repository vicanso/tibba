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

use axum::Json;
use axum::Router;
use axum::extract::Multipart;
use axum::extract::State;
use axum::http::header;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use imageoptimize::{ImageProcessingError, ProcessImage, run_with_image};
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, Snafu};
use sqlx::PgPool;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tibba_error::Error as BaseError;
use tibba_model_builtin::{ConfigurationModel, FileInsertParams, FileModel, Model};
use tibba_opendal::{PresignResult, Storage};
use tibba_session::UserSession;
use tibba_util::{JsonResult, QueryParams, uuid};
use tibba_validator::{x_file_group, x_file_name, x_image_format, x_image_quality};
use utoipa::{IntoParams, OpenApi, ToSchema};
use validator::Validate;

/// 模块对外仍返回 `tibba_error::Error`，本地 `Error` 仅用于 snafu 上下文捕获。
type Result<T, E = BaseError> = std::result::Result<T, E>;

const ERROR_CATEGORY: &str = "file_router";

/// 文件路由模块内部错误，统一通过 `From` 转换为 `tibba_error::Error`。
#[derive(Debug, Snafu)]
enum Error {
    /// 数据库中找不到指定文件（HTTP 404）
    #[snafu(display("file not found: {name}"))]
    FileNotFound { name: String },

    /// multipart 表单字段读取失败
    #[snafu(display("multipart read fail: {source}"))]
    Multipart {
        source: axum::extract::multipart::MultipartError,
    },

    /// 图片格式探测 / 解码失败（来自 `image` crate）
    #[snafu(display("image decode fail: {source}"))]
    Image {
        // `image::ImageError` 体积较大，统一装箱避免 enum 膨胀
        #[snafu(source(from(image::ImageError, Box::new)))]
        source: Box<image::ImageError>,
    },

    /// 图片优化任务失败（来自 `imageoptimize` crate）
    #[snafu(display("optimize fail: {source}"))]
    Optimize {
        // `ImageProcessingError` 内含 image / io / utf 多源，体积较大需装箱
        #[snafu(source(from(ImageProcessingError, Box::new)))]
        source: Box<ImageProcessingError>,
    },

    /// 预签名上传的文件名缺少扩展名，无法据此生成存储键（HTTP 400）
    #[snafu(display("invalid filename (missing extension): {name}"))]
    InvalidFilename { name: String },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::FileNotFound { name } => BaseError::new(format!("file not found: {name}"))
                .with_sub_category("not_found")
                .with_status(404)
                .with_exception(false),
            Error::Multipart { source } => BaseError::new(source).with_sub_category("multipart"),
            Error::Image { source } => BaseError::new(source).with_sub_category("image"),
            Error::Optimize { source } => BaseError::new(source).with_sub_category("optimize"),
            Error::InvalidFilename { name } => {
                BaseError::new(format!("invalid filename (missing extension): {name}"))
                    .with_sub_category("invalid_filename")
                    .with_status(400)
                    .with_exception(false)
            }
        };
        err.with_category(ERROR_CATEGORY)
    }
}

#[derive(Debug, Deserialize, Clone, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
struct CreateFileParams {
    /// 文件所属分组（决定存储路径与响应头策略）
    #[validate(custom(function = "x_file_group"))]
    group: String,
}

#[utoipa::path(
    post,
    path = "/files/upload",
    tag = "file",
    params(CreateFileParams),
    request_body(content_type = "multipart/form-data", description = "一个或多个文件字段（multipart/form-data）"),
    responses(
        (status = 200, description = "上传成功，返回 字段名 → 存储文件名 的映射", body = HashMap<String, String>)
    )
)]
async fn create_file(
    QueryParams(create_file_params): QueryParams<CreateFileParams>,
    State((storage, pool)): State<(&'static Storage, &'static PgPool)>,
    session: UserSession,
    mut multipart: Multipart,
) -> JsonResult<HashMap<String, String>> {
    let mut files = HashMap::new();
    while let Some(mut field) = multipart.next_field().await.context(MultipartSnafu)? {
        let name = field.name().unwrap_or_default().to_string();
        let file_name = field.file_name().unwrap_or_default().to_string();
        if name.is_empty() && file_name.is_empty() {
            continue;
        }
        let ext = Path::new(&file_name)
            .extension()
            .unwrap_or_default()
            .to_string_lossy();
        if ext.is_empty() {
            continue;
        }
        let file = format!("{}.{}", uuid(), ext);

        // 先按文件名猜测类型：图片需整图缓冲以解码宽高，其余大文件走流式直写
        let guessed = mime_guess2::from_path(&file_name).first_or_octet_stream();
        let mut params = FileInsertParams {
            group: create_file_params.group.clone(),
            filename: file.clone(),
            content_type: guessed.to_string(),
            uploader: session.get_account().to_string(),
            ..Default::default()
        };

        if guessed.type_() == "image" {
            // 图片：需解码取宽高，且预览/优化链路本就要整图，故缓冲读取（图片体积有界）
            let data = field.bytes().await.context(MultipartSnafu)?;
            let format = image::guess_format(&data).context(ImageSnafu)?;
            params.content_type = format.to_mime_type().to_string();
            let image = image::load_from_memory_with_format(&data, format).context(ImageSnafu)?;
            params.width = Some(image.width() as i32);
            params.height = Some(image.height() as i32);
            params.file_size = data.len() as i64;
            storage.write_with(&file, data, vec![]).await?;
        } else {
            // 非图片（视频 / 归档等大文件）：边收边写，内存占用恒定，避免整文件入内存 OOM
            let mut writer = storage.writer(&file).await?;
            let mut size: i64 = 0;
            while let Some(chunk) = field.chunk().await.context(MultipartSnafu)? {
                size += chunk.len() as i64;
                if let Err(e) = writer.write(chunk).await {
                    // 写入失败：尽量中止以清理半截对象，再上抛
                    let _ = writer.abort().await;
                    return Err(e.into());
                }
            }
            writer.close().await?;
            params.file_size = size;
        }

        FileModel::new().insert_file(pool, params).await?;

        files.insert(name, file);
    }
    Ok(Json(files))
}

#[derive(Debug, Deserialize, Clone, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
struct GetFileParams {
    /// 存储文件名
    #[validate(custom(function = "x_file_name"))]
    name: String,
    /// 目标图片格式（如 webp/avif），缺省沿用原格式
    #[validate(custom(function = "x_image_format"))]
    format: Option<String>,
    /// 图片质量 1-100，缺省 80
    #[validate(custom(function = "x_image_quality"))]
    quality: Option<u8>,
    /// 是否对图片做优化/压缩，缺省 true
    optimize: Option<bool>,
    /// 缩放目标宽度（像素）
    width: Option<u32>,
    /// 缩放目标高度（像素）
    height: Option<u32>,
}

#[utoipa::path(
    get,
    path = "/files/preview",
    tag = "file",
    params(GetFileParams),
    responses(
        (status = 200, description = "文件内容（图片可按参数优化/缩放），二进制流"),
        (status = 404, description = "文件不存在")
    )
)]
async fn get_file(
    State((storage, pool)): State<(&'static Storage, &'static PgPool)>,
    QueryParams(params): QueryParams<GetFileParams>,
) -> Result<impl IntoResponse> {
    let file = FileModel::new()
        .get_by_name(pool, &params.name)
        .await?
        .context(FileNotFoundSnafu {
            name: params.name.clone(),
        })?;
    let mut data = storage.read(&params.name).await?;
    let mut content_type = file.content_type.clone();
    // mime 形如 `image/png`，取斜杠后一段作为扩展名；无斜杠时返回原串
    let ext = content_type.split('/').next_back().unwrap_or_default();
    let format = params.format.clone().unwrap_or_else(|| ext.to_string());
    let mut headers = header::HeaderMap::with_capacity(8);
    if params.optimize.unwrap_or(true) && content_type.starts_with("image") {
        let image = ProcessImage::new(data.to_vec(), ext).context(OptimizeSnafu)?;
        let mut tasks = vec![];
        if params.width.is_some() || params.height.is_some() {
            tasks.push(vec![
                "resize".to_string(),
                params.width.unwrap_or(0).to_string(),
                params.height.unwrap_or(0).to_string(),
            ]);
        }
        tasks.push(vec![
            "optim".to_string(),
            format.clone(),
            params.quality.unwrap_or(80).to_string(),
            "3".to_string(),
        ]);
        tasks.push(vec!["diff".to_string()]);

        let image = run_with_image(image, tasks).await.context(OptimizeSnafu)?;

        if let Ok(diff) = HeaderValue::from_str(&format!("{:.2}", image.diff)) {
            headers.insert("X-Diff", diff);
        }

        data = image
            .get_buffer()
            .context(OptimizeSnafu)?
            .into_owned()
            .into();
        if let Some(mime_type) = mime_guess2::from_ext(format.as_str()).first() {
            content_type = mime_type.to_string();
        }
    }

    if let Ok(header_value) = HeaderValue::from_str(&content_type) {
        headers.insert(header::CONTENT_TYPE, header_value);
    }
    let size = data.len();
    if size > 0 {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    }
    if let Some(metadata) = file.get_metadata() {
        headers.extend(metadata);
    }
    if let Some(response_headers) = ConfigurationModel::new()
        .get_response_headers(pool, &file.group)
        .await?
    {
        headers.extend(response_headers);
    }
    Ok((headers, data.to_bytes()))
}

/// 单段 HTTP `Range` 解析结果。
enum RangeSpec {
    /// 无 Range / 语法无法识别 / 多段：按整文件返回（200）。
    Full,
    /// 可满足的闭区间 `[start, end]`（206）。
    Partial(u64, u64),
    /// 语法合法但区间越界：不可满足（416）。
    Unsatisfiable,
}

/// 解析单段 `Range: bytes=...` 头。仅支持单段，多段（含逗号）退化为整文件。
/// 支持 `bytes=start-end` / `bytes=start-`（到末尾）/ `bytes=-suffix`（最后 N 字节）。
fn parse_byte_range(value: &str, total: u64) -> RangeSpec {
    let Some(spec) = value.trim().strip_prefix("bytes=") else {
        return RangeSpec::Full;
    };
    // 多段范围不支持，退化为整文件返回
    if spec.contains(',') {
        return RangeSpec::Full;
    }
    let Some((start_str, end_str)) = spec.split_once('-') else {
        return RangeSpec::Full;
    };
    let (start_str, end_str) = (start_str.trim(), end_str.trim());
    // 空文件无法满足任何非空范围
    if total == 0 {
        return RangeSpec::Unsatisfiable;
    }
    let (start, end) = if start_str.is_empty() {
        // 后缀范围 bytes=-N：返回最后 N 字节
        let Ok(n) = end_str.parse::<u64>() else {
            return RangeSpec::Full;
        };
        if n == 0 {
            return RangeSpec::Unsatisfiable;
        }
        let n = n.min(total);
        (total - n, total - 1)
    } else {
        let Ok(start) = start_str.parse::<u64>() else {
            return RangeSpec::Full;
        };
        // end 缺省为末字节；显式给出时夹取到末字节
        let end = if end_str.is_empty() {
            total - 1
        } else {
            match end_str.parse::<u64>() {
                Ok(e) => e.min(total - 1),
                Err(_) => return RangeSpec::Full,
            }
        };
        (start, end)
    };
    if start > end || start >= total {
        return RangeSpec::Unsatisfiable;
    }
    RangeSpec::Partial(start, end)
}

#[derive(Debug, Deserialize, Clone, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
struct DownloadFileParams {
    /// 存储对象键
    #[validate(custom(function = "x_file_name"))]
    name: String,
}

/// `GET /files/download` —— 原样下载对象，支持 HTTP `Range`（断点续传 / 视频拖动）。
///
/// 与 `/files/preview` 的区别：preview 面向图片做格式转换 / 缩放 / 优化（返回的是
/// 变换后的字节，无法 Range）；download 返回存储中的原始字节，可按 `Range` 请求分片。
/// 仅对数据库中登记过的对象提供服务，未登记直接 404。
#[utoipa::path(
    get,
    path = "/files/download",
    tag = "file",
    params(DownloadFileParams),
    responses(
        (status = 200, description = "完整文件内容（二进制流）"),
        (status = 206, description = "Range 请求的部分内容"),
        (status = 404, description = "文件不存在"),
        (status = 416, description = "Range 不可满足")
    )
)]
async fn download_file(
    State((storage, pool)): State<(&'static Storage, &'static PgPool)>,
    headers: HeaderMap,
    QueryParams(params): QueryParams<DownloadFileParams>,
) -> Result<impl IntoResponse> {
    // 仅对已登记对象服务：未登记直接 404，避免被用来探测存储桶
    let file = FileModel::new()
        .get_by_name(pool, &params.name)
        .await?
        .context(FileNotFoundSnafu {
            name: params.name.clone(),
        })?;
    let total = storage.stat(&params.name).await?.content_length();

    let mut resp_headers = header::HeaderMap::with_capacity(8);
    // 声明支持 Range，客户端据此发起分片请求
    resp_headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Ok(content_type) = HeaderValue::from_str(&file.content_type) {
        resp_headers.insert(header::CONTENT_TYPE, content_type);
    }
    if let Some(metadata) = file.get_metadata() {
        resp_headers.extend(metadata);
    }
    if let Some(response_headers) = ConfigurationModel::new()
        .get_response_headers(pool, &file.group)
        .await?
    {
        resp_headers.extend(response_headers);
    }

    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(|v| parse_byte_range(v, total))
        .unwrap_or(RangeSpec::Full);

    match range {
        RangeSpec::Partial(start, end) => {
            // HTTP Range 为闭区间，长度 = end - start + 1
            let len = end - start + 1;
            let data = storage.read_range(&params.name, start, len).await?;
            resp_headers.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
            if let Ok(content_range) =
                HeaderValue::from_str(&format!("bytes {start}-{end}/{total}"))
            {
                resp_headers.insert(header::CONTENT_RANGE, content_range);
            }
            Ok((StatusCode::PARTIAL_CONTENT, resp_headers, data.to_bytes()).into_response())
        }
        RangeSpec::Unsatisfiable => {
            // 416：按规范回 `Content-Range: bytes */total` 告知合法总长
            if let Ok(content_range) = HeaderValue::from_str(&format!("bytes */{total}")) {
                resp_headers.insert(header::CONTENT_RANGE, content_range);
            }
            Ok((StatusCode::RANGE_NOT_SATISFIABLE, resp_headers).into_response())
        }
        RangeSpec::Full => {
            let data = storage.read(&params.name).await?;
            resp_headers.insert(header::CONTENT_LENGTH, HeaderValue::from(total));
            Ok((resp_headers, data.to_bytes()).into_response())
        }
    }
}

/// 预签名 URL 有效期：15 分钟，够客户端完成一次直传/直下，又不过长暴露。
const PRESIGN_EXPIRE: Duration = Duration::from_secs(15 * 60);

/// 预签名请求响应：客户端据此直接向存储后端发起上传/下载，不经应用中转。
#[derive(Debug, Serialize, ToSchema)]
struct PresignResp {
    /// 存储对象键（上传为服务端新分配的 `{uuid}.{ext}`，下载即请求的 name）
    key: String,
    /// HTTP 方法：上传 `PUT`，下载 `GET`
    method: String,
    /// 预签名 URL（含鉴权参数，到期失效）
    url: String,
    /// 客户端发起请求时须带上的头（如 host）
    headers: HashMap<String, String>,
    /// 有效期（秒）
    expires_in: u64,
}

/// 把存储层 [`PresignResult`] 连同对象键组装成 API 响应。
fn presign_resp(key: String, result: PresignResult) -> PresignResp {
    PresignResp {
        key,
        method: result.method,
        url: result.url,
        headers: result.headers.into_iter().collect(),
        expires_in: PRESIGN_EXPIRE.as_secs(),
    }
}

#[derive(Debug, Deserialize, Clone, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
struct PresignUploadParams {
    /// 文件所属分组（决定存储路径策略）
    #[validate(custom(function = "x_file_group"))]
    group: String,
    /// 原始文件名，仅用于提取扩展名；最终存储键为 `{uuid}.{ext}`
    #[validate(length(min = 1, max = 255))]
    filename: String,
}

/// `GET /files/presign/upload` —— 为「直传」签发预签名 PUT URL。
///
/// 仅登录用户可调用。服务端按扩展名分配存储键并返回预签名 URL，客户端凭此
/// 直接 PUT 到对象存储，无需经应用中转。仅 S3 等支持 presign 的后端可用。
#[utoipa::path(
    get,
    path = "/files/presign/upload",
    tag = "file",
    params(PresignUploadParams),
    responses(
        (status = 200, description = "返回预签名上传请求（key/method/url/headers/expires_in）", body = PresignResp),
        (status = 400, description = "文件名缺少扩展名"),
        (status = 401, description = "未登录")
    )
)]
async fn presign_upload(
    State((storage, _pool)): State<(&'static Storage, &'static PgPool)>,
    _session: UserSession,
    QueryParams(params): QueryParams<PresignUploadParams>,
) -> JsonResult<PresignResp> {
    let ext = Path::new(&params.filename)
        .extension()
        .unwrap_or_default()
        .to_string_lossy();
    if ext.is_empty() {
        return Err(Error::InvalidFilename {
            name: params.filename.clone(),
        }
        .into());
    }
    let key = format!("{}.{}", uuid(), ext);
    let result = storage.presign_write(&key, PRESIGN_EXPIRE).await?;
    Ok(Json(presign_resp(key, result)))
}

#[derive(Debug, Deserialize, Clone, Validate, IntoParams)]
#[into_params(parameter_in = Query)]
struct PresignDownloadParams {
    /// 存储对象键
    #[validate(custom(function = "x_file_name"))]
    name: String,
}

/// `GET /files/presign/download` —— 为「直下」签发预签名 GET URL。
///
/// 仅登录用户可调用，且仅对数据库中登记过的对象签名（杜绝对任意键的探测）。
/// 仅 S3 等支持 presign 的后端可用。
#[utoipa::path(
    get,
    path = "/files/presign/download",
    tag = "file",
    params(PresignDownloadParams),
    responses(
        (status = 200, description = "返回预签名下载请求（key/method/url/headers/expires_in）", body = PresignResp),
        (status = 401, description = "未登录"),
        (status = 404, description = "对象不存在")
    )
)]
async fn presign_download(
    State((storage, pool)): State<(&'static Storage, &'static PgPool)>,
    _session: UserSession,
    QueryParams(params): QueryParams<PresignDownloadParams>,
) -> JsonResult<PresignResp> {
    // 仅对已登记对象签名：未登记直接 404，避免被用来探测存储桶
    FileModel::new()
        .get_by_name(pool, &params.name)
        .await?
        .context(FileNotFoundSnafu {
            name: params.name.clone(),
        })?;
    let result = storage.presign_read(&params.name, PRESIGN_EXPIRE).await?;
    Ok(Json(presign_resp(params.name, result)))
}

pub struct FileRouterParams {
    pub storage: &'static Storage,
    pub pool: &'static PgPool,
}

pub fn new_file_router(params: FileRouterParams) -> Router {
    Router::new()
        .route(
            "/upload",
            post(create_file).with_state((params.storage, params.pool)),
        )
        .route(
            "/preview",
            get(get_file).with_state((params.storage, params.pool)),
        )
        .route(
            "/download",
            get(download_file).with_state((params.storage, params.pool)),
        )
        .route(
            "/presign/upload",
            get(presign_upload).with_state((params.storage, params.pool)),
        )
        .route(
            "/presign/download",
            get(presign_download).with_state((params.storage, params.pool)),
        )
}

/// 本路由模块的 OpenAPI 文档片段（路径相对 `/files` 已在注解里写全）。
#[derive(OpenApi)]
#[openapi(
    paths(create_file, get_file, download_file, presign_upload, presign_download),
    components(schemas(PresignResp)),
    tags((name = "file", description = "文件上传与预览"))
)]
struct FileApiDoc;

/// 返回 file 路由的 OpenAPI 文档片段，供主 crate 合并进全局文档。
pub fn openapi() -> utoipa::openapi::OpenApi {
    FileApiDoc::openapi()
}

#[cfg(test)]
mod tests {
    use super::{RangeSpec, parse_byte_range};

    /// 断言解析结果为指定闭区间。
    fn assert_partial(value: &str, total: u64, start: u64, end: u64) {
        match parse_byte_range(value, total) {
            RangeSpec::Partial(s, e) => assert_eq!((s, e), (start, end), "value={value}"),
            other => panic!("expected Partial for {value:?}, got {other:?}"),
        }
    }

    /// 便于 panic 信息打印的简单 Debug。
    impl std::fmt::Debug for RangeSpec {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                RangeSpec::Full => write!(f, "Full"),
                RangeSpec::Partial(s, e) => write!(f, "Partial({s},{e})"),
                RangeSpec::Unsatisfiable => write!(f, "Unsatisfiable"),
            }
        }
    }

    #[test]
    fn normal_closed_range() {
        // bytes=0-499 → [0,499]
        assert_partial("bytes=0-499", 1000, 0, 499);
        // 末字节夹取到 total-1
        assert_partial("bytes=500-100000", 1000, 500, 999);
    }

    #[test]
    fn open_ended_range() {
        // bytes=500- → [500, total-1]
        assert_partial("bytes=500-", 1000, 500, 999);
    }

    #[test]
    fn suffix_range() {
        // bytes=-200 → 最后 200 字节
        assert_partial("bytes=-200", 1000, 800, 999);
        // 后缀超过总长 → 夹取为整文件
        assert_partial("bytes=-5000", 1000, 0, 999);
    }

    #[test]
    fn full_when_no_or_bad_range() {
        // 无 bytes= 前缀
        assert!(matches!(parse_byte_range("items=0-1", 1000), RangeSpec::Full));
        // 多段不支持，退化整文件
        assert!(matches!(
            parse_byte_range("bytes=0-1,2-3", 1000),
            RangeSpec::Full
        ));
        // 非数字
        assert!(matches!(parse_byte_range("bytes=a-b", 1000), RangeSpec::Full));
    }

    #[test]
    fn unsatisfiable_range() {
        // start 越界
        assert!(matches!(
            parse_byte_range("bytes=1000-1100", 1000),
            RangeSpec::Unsatisfiable
        ));
        // 空文件
        assert!(matches!(
            parse_byte_range("bytes=0-0", 0),
            RangeSpec::Unsatisfiable
        ));
        // 后缀 0 字节
        assert!(matches!(
            parse_byte_range("bytes=-0", 1000),
            RangeSpec::Unsatisfiable
        ));
    }
}
