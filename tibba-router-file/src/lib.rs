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
use axum::extract::DefaultBodyLimit;
use axum::extract::FromRequestParts;
use axum::extract::Multipart;
use axum::extract::State;
use axum::http::header;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use imageoptimize::{ImageProcessingError, ProcessImage, run_with_image};
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, Snafu};
use sqlx::PgPool;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;
use tibba_error::Error as BaseError;
use tibba_model_builtin::{ConfigurationModel, File, FileInsertParams, FileModel, Model};
use tibba_opendal::{PresignResult, Storage};
use tibba_session::{Session, UserSession};
use tibba_util::{
    JsonParams, JsonResult, QueryParams, uuid, x_file_group, x_file_name, x_image_format,
    x_image_quality,
};
use utoipa::{IntoParams, OpenApi, ToSchema};
use validator::Validate;

/// 模块对外仍返回 `tibba_error::Error`，本地 `Error` 仅用于 snafu 上下文捕获。
type Result<T, E = BaseError> = std::result::Result<T, E>;

const ERROR_CATEGORY: &str = "file_router";

/// 单次上传单个文件的内容上限（50 MiB）。流式分支累计超过即中止写入并返回 413；
/// 图片分支按此上限收集，避免超大「图片」撑爆内存。部署如需更大可调整此常量。
const MAX_UPLOAD_BYTES: usize = 50 * 1024 * 1024;

/// 栅格图片宽高与缩放目标上限（像素），防止解压炸弹 / 恶意超大缩放耗尽内存。
const MAX_IMAGE_DIMENSION: u32 = 4096;
/// 允许的文件扩展名最大长度。
const MAX_EXTENSION_LEN: usize = 16;

/// 单次请求最多处理的文件字段数，防海量小字段的 multipart 请求耗尽资源。
const MAX_UPLOAD_FILES: usize = 20;
/// 单次请求所有字段累计字节上限（200 MiB），叠加于每字段 `MAX_UPLOAD_BYTES` 之上，
/// 防多个近上限字段合计撑爆存储 / 数据库。
const MAX_UPLOAD_TOTAL_BYTES: i64 = 200 * 1024 * 1024;

/// 文件路由模块内部错误，统一通过 `From` 转换为 `tibba_error::Error`。
#[derive(Debug, Snafu)]
enum Error {
    /// 数据库中找不到指定文件（HTTP 404）
    #[snafu(display("file not found: {name}"))]
    FileNotFound { name: String },

    /// multipart 表单字段读取失败（HTTP 400）
    #[snafu(display("multipart read fail: {source}"))]
    Multipart {
        source: axum::extract::multipart::MultipartError,
    },

    /// 图片格式探测 / 解码失败（来自 `image` crate，HTTP 400）
    #[snafu(display("image decode fail: {source}"))]
    Image {
        // `image::ImageError` 体积较大，统一装箱避免 enum 膨胀
        #[snafu(source(from(image::ImageError, Box::new)))]
        source: Box<image::ImageError>,
    },

    /// 图片尺寸超限或格式无效（HTTP 400）
    #[snafu(display("{message}"))]
    ImageDimensions { message: String },

    /// 图片优化任务失败（来自 `imageoptimize` crate）
    #[snafu(display("optimize fail: {source}"))]
    Optimize {
        // `ImageProcessingError` 内含 image / io / utf 多源，体积较大需装箱
        #[snafu(source(from(ImageProcessingError, Box::new)))]
        source: Box<ImageProcessingError>,
    },

    /// 预签名或上传的文件名/扩展名不合法（HTTP 400）
    #[snafu(display("invalid filename: {name}"))]
    InvalidFilename { name: String },

    /// 预签名确认键不合法（HTTP 400）
    #[snafu(display("invalid presign key: {key}"))]
    InvalidPresignKey { key: String },

    /// 文件记录已存在（HTTP 409）
    #[snafu(display("file already exists: {name}"))]
    FileAlreadyExists { name: String },

    /// 无权访问私有分组文件（HTTP 403）
    #[snafu(display("forbidden to access private file"))]
    Forbidden,

    /// 上传内容超过单文件上限（HTTP 413）
    #[snafu(display("upload too large (max {max} bytes)"))]
    PayloadTooLarge { max: usize },
    /// 单次请求文件字段数超过上限（HTTP 413）
    #[snafu(display("too many files in one request (max {max})"))]
    TooManyFiles { max: usize },
}

impl From<Error> for BaseError {
    fn from(val: Error) -> Self {
        let err = match val {
            Error::FileNotFound { name } => BaseError::new(format!("file not found: {name}"))
                .with_sub_category("not_found")
                .with_status(404)
                .with_exception(false),
            Error::Multipart { source } => BaseError::new(source)
                .with_sub_category("multipart")
                .with_status(400)
                .with_exception(false),
            Error::Image { source } => BaseError::new(source)
                .with_sub_category("image")
                .with_status(400)
                .with_exception(false),
            Error::ImageDimensions { message } => BaseError::new(message)
                .with_sub_category("image_dimensions")
                .with_status(400)
                .with_exception(false),
            Error::Optimize { source } => {
                let is_client_err = matches!(
                    *source,
                    ImageProcessingError::ParamsInvalid { .. } | ImageProcessingError::Image { .. }
                );
                let base = BaseError::new(source).with_sub_category("optimize");
                if is_client_err {
                    base.with_status(400).with_exception(false)
                } else {
                    base.with_exception(true)
                }
            }
            Error::InvalidFilename { name } => BaseError::new(format!("invalid filename: {name}"))
                .with_sub_category("invalid_filename")
                .with_status(400)
                .with_exception(false),
            Error::InvalidPresignKey { key } => {
                BaseError::new(format!("invalid presign key: {key}"))
                    .with_sub_category("invalid_presign_key")
                    .with_status(400)
                    .with_exception(false)
            }
            Error::FileAlreadyExists { name } => {
                BaseError::new(format!("file already exists: {name}"))
                    .with_sub_category("already_exists")
                    .with_status(409)
                    .with_exception(false)
            }
            Error::Forbidden => BaseError::new("forbidden to access private file")
                .with_sub_category("forbidden")
                .with_status(403)
                .with_exception(false),
            Error::PayloadTooLarge { max } => {
                BaseError::new(format!("upload too large (max {max} bytes)"))
                    .with_sub_category("payload_too_large")
                    .with_status(413)
                    .with_exception(false)
            }
            Error::TooManyFiles { max } => {
                BaseError::new(format!("too many files in one request (max {max})"))
                    .with_sub_category("too_many_files")
                    .with_status(413)
                    .with_exception(false)
            }
        };
        err.with_category(ERROR_CATEGORY)
    }
}

/// 可选 Session 提取器：未挂载 Session 中间件或未带凭证时回退为 `None`。
struct OptionalSession(Option<Session>);

impl<S> FromRequestParts<S> for OptionalSession
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        Ok(Self(Session::from_request_parts(parts, state).await.ok()))
    }
}

/// 校验并归一化文件扩展名（仅允许 1..=16 位 ASCII 字母或数字，返回小写）。
fn normalize_extension(file_name: &str) -> Result<String> {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .trim();
    if ext.is_empty()
        || ext.len() > MAX_EXTENSION_LEN
        || !ext.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err(Error::InvalidFilename {
            name: file_name.to_string(),
        }
        .into());
    }
    Ok(ext.to_ascii_lowercase())
}

/// 判断是否为纯安全栅格图片（显式排除 `image/svg+xml`）。
fn is_raster_image(content_type: &str) -> bool {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    matches!(
        essence.as_str(),
        "image/png"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "image/avif"
            | "image/bmp"
            | "image/tiff"
    )
}

/// 仅解码图片头校验格式与宽高，阻止解压炸弹（`1..=4096`）。
fn inspect_raster_dimensions(data: &[u8]) -> Result<(image::ImageFormat, u32, u32)> {
    let reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| Error::ImageDimensions {
            message: format!("sniff image format fail: {e}"),
        })?;
    let format = reader.format().ok_or_else(|| Error::ImageDimensions {
        message: "unknown or unsupported raster image format".to_string(),
    })?;
    let (w, h) = reader.into_dimensions().context(ImageSnafu)?;
    if w == 0 || h == 0 || w > MAX_IMAGE_DIMENSION || h > MAX_IMAGE_DIMENSION {
        return Err(Error::ImageDimensions {
            message: format!("image dimensions {w}x{h} exceed 1..={MAX_IMAGE_DIMENSION}"),
        }
        .into());
    }
    Ok((format, w, h))
}

/// 私有分组鉴权：仅上传者本人或管理员可访问 `private` 分组文件。
fn authorize_file_access(file: &File, session: Option<&Session>) -> Result<()> {
    if file.group != "private" {
        return Ok(());
    }
    let Some(session) = session.filter(|s| s.is_login()) else {
        return Err(tibba_session::Error::UserNotLogin.into());
    };
    if session.get_account() == file.uploader || session.is_admin() {
        return Ok(());
    }
    Err(Error::Forbidden.into())
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
    // 请求级配额：字段数与累计字节，防多字段绕过单字段上限耗尽资源
    let mut file_count: usize = 0;
    let mut total_bytes: i64 = 0;
    while let Some(mut field) = multipart.next_field().await.context(MultipartSnafu)? {
        let name = field.name().unwrap_or_default().to_string();
        let file_name = field.file_name().unwrap_or_default().to_string();
        if name.is_empty() || file_name.is_empty() {
            continue;
        }
        let ext = normalize_extension(&file_name)?;
        file_count += 1;
        if file_count > MAX_UPLOAD_FILES {
            return Err(Error::TooManyFiles {
                max: MAX_UPLOAD_FILES,
            }
            .into());
        }
        let file = format!("{}.{}", uuid(), ext);

        // 先按文件名猜测类型：仅对栅格图片缓冲并用 header 解码宽高（排除 SVG），其余大文件走流式直写
        let guessed = mime_guess2::from_path(&file_name).first_or_octet_stream();
        let mut params = FileInsertParams {
            group: create_file_params.group.clone(),
            filename: file.clone(),
            content_type: guessed.to_string(),
            uploader: session.get_account().to_string(),
            ..Default::default()
        };

        if is_raster_image(guessed.essence_str()) {
            // 栅格图片：按上限收集并用 ImageReader::into_dimensions 只读文件头防解压炸弹
            let mut data = Vec::new();
            while let Some(chunk) = field.chunk().await.context(MultipartSnafu)? {
                let projected = data.len() + chunk.len();
                if projected > MAX_UPLOAD_BYTES {
                    return Err(Error::PayloadTooLarge {
                        max: MAX_UPLOAD_BYTES,
                    }
                    .into());
                }
                // 请求累计（含本字段在途字节）超上限：图片尚在内存缓冲、未落存储，直接拒
                if total_bytes + projected as i64 > MAX_UPLOAD_TOTAL_BYTES {
                    return Err(Error::PayloadTooLarge {
                        max: MAX_UPLOAD_TOTAL_BYTES as usize,
                    }
                    .into());
                }
                data.extend_from_slice(&chunk);
            }
            let (format, width, height) = inspect_raster_dimensions(&data)?;
            params.content_type = format.to_mime_type().to_string();
            params.width = Some(width as i32);
            params.height = Some(height as i32);
            params.file_size = data.len() as i64;
            storage.write_with(&file, data, vec![]).await?;
        } else {
            // 非栅格图片（SVG / 视频 / 归档等）：边收边写，内存占用恒定，避免整文件 OOM
            let mut writer = storage.writer(&file).await?;
            let mut size: i64 = 0;
            loop {
                let chunk = match field.chunk().await {
                    Ok(Some(c)) => c,
                    Ok(None) => break,
                    Err(e) => {
                        let _ = writer.abort().await;
                        return Err(Error::Multipart { source: e }.into());
                    }
                };
                size += chunk.len() as i64;
                // 单字段或请求累计超上限：中止写入（清理半截对象）并返回 413，不再继续收流
                if size > MAX_UPLOAD_BYTES as i64 {
                    let _ = writer.abort().await;
                    return Err(Error::PayloadTooLarge {
                        max: MAX_UPLOAD_BYTES,
                    }
                    .into());
                }
                if total_bytes + size > MAX_UPLOAD_TOTAL_BYTES {
                    let _ = writer.abort().await;
                    return Err(Error::PayloadTooLarge {
                        max: MAX_UPLOAD_TOTAL_BYTES as usize,
                    }
                    .into());
                }
                if let Err(e) = writer.write(chunk).await {
                    // 写入失败：尽量中止以清理半截对象，再上抛
                    let _ = writer.abort().await;
                    return Err(e.into());
                }
            }
            if let Err(e) = writer.close().await {
                let _ = storage.delete(&file).await;
                return Err(e.into());
            }
            params.file_size = size;
        }

        // 累加本字段实际字节；若写库失败则删除存储中的孤儿对象后再返回错误
        let field_size = params.file_size;
        if let Err(err) = FileModel::new().insert_file(pool, params).await {
            let _ = storage.delete(&file).await;
            return Err(err.into());
        }
        total_bytes += field_size;

        files.insert(name, file);
    }
    Ok(Json(files))
}

/// 决定响应的 `Content-Disposition`：仅安全栅格图片允许 `inline` 渲染，其余（含 SVG）一律 `attachment`。
fn content_disposition_for(content_type: &str) -> &'static str {
    if is_raster_image(content_type) {
        "inline"
    } else {
        "attachment"
    }
}

/// 在响应头的**最后**写入决定「内容如何被解释」的头，保证不被数据驱动的头覆盖。
///
/// 下载 / 预览会合并文件元数据与分组配置里的响应头。此前安全相关头先写、合并
/// 后写，而 `HeaderMap::extend` 对同名头是替换语义——数据里一条
/// `Content-Disposition: inline` 就能把这里的防护整个抵消。模型层已拒绝这类
/// 头（`is_data_overridable_response_header`），这里再按「最后写者赢」兜一层。
///
/// `Content-Security-Policy: sandbox`：即便某种类型被浏览器渲染成文档，也会被
/// 当作不透明源、禁止脚本执行，与应用同源隔离开。对 `<img>` 引用无影响。
fn apply_file_security_headers(headers: &mut header::HeaderMap, content_type: &str) {
    if let Ok(value) = HeaderValue::from_str(content_type) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static(content_disposition_for(content_type)),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
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
    /// 缩放目标宽度（像素，1..=4096）
    #[validate(range(min = 1, max = 4096))]
    width: Option<u32>,
    /// 缩放目标高度（像素，1..=4096）
    #[validate(range(min = 1, max = 4096))]
    height: Option<u32>,
}

#[utoipa::path(
    get,
    path = "/files/preview",
    tag = "file",
    params(GetFileParams),
    responses(
        (status = 200, description = "文件内容（图片可按参数优化/缩放），二进制流"),
        (status = 403, description = "无权访问私有分组文件"),
        (status = 404, description = "文件不存在")
    )
)]
async fn get_file(
    State((storage, pool)): State<(&'static Storage, &'static PgPool)>,
    session: OptionalSession,
    QueryParams(params): QueryParams<GetFileParams>,
) -> Result<impl IntoResponse> {
    let file = FileModel::new()
        .get_by_name(pool, &params.name)
        .await?
        .context(FileNotFoundSnafu {
            name: params.name.clone(),
        })?;
    authorize_file_access(&file, session.0.as_ref())?;
    let mut data = storage.read(&params.name).await?;
    let mut content_type = file.content_type.clone();
    // mime 形如 `image/png`，取斜杠后一段作为扩展名；无斜杠时返回原串
    let ext = content_type
        .split('/')
        .next_back()
        .unwrap_or_default()
        .to_string();
    let format = params.format.clone().unwrap_or_else(|| ext.clone());
    let mut headers = header::HeaderMap::with_capacity(8);
    if params.optimize.unwrap_or(true) && is_raster_image(&content_type) {
        let raw = data.to_vec();
        let _ = inspect_raster_dimensions(&raw)?;
        let mut tasks = vec![];
        if params.width.is_some() || params.height.is_some() {
            let w = params.width.unwrap_or(0).min(MAX_IMAGE_DIMENSION);
            let h = params.height.unwrap_or(0).min(MAX_IMAGE_DIMENSION);
            tasks.push(vec!["resize".to_string(), w.to_string(), h.to_string()]);
        }
        tasks.push(vec![
            "optim".to_string(),
            format.clone(),
            params.quality.unwrap_or(80).to_string(),
            "3".to_string(),
        ]);
        tasks.push(vec!["diff".to_string()]);

        let handle = tokio::runtime::Handle::current();
        let (optimized, diff_str) = tokio::task::spawn_blocking(move || {
            let image = ProcessImage::new(raw, &ext).context(OptimizeSnafu)?;
            let image = handle
                .block_on(run_with_image(image, tasks))
                .context(OptimizeSnafu)?;
            let diff_str = format!("{:.2}", image.diff);
            let buf = image.get_buffer().context(OptimizeSnafu)?.into_owned();
            Ok::<(Vec<u8>, String), BaseError>((buf, diff_str))
        })
        .await
        .map_err(|e| {
            BaseError::new(format!("image optimize join fail: {e}"))
                .with_category(ERROR_CATEGORY)
                .with_exception(true)
        })??;

        if let Ok(diff) = HeaderValue::from_str(&diff_str) {
            headers.insert("X-Diff", diff);
        }

        data = optimized.into();
        if let Some(mime_type) = mime_guess2::from_ext(format.as_str()).first() {
            content_type = mime_type.to_string();
        }
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
    // 使用优化变换后的真实字节数写回 Content-Length，覆盖任何过期/数据驱动头
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(data.len()));
    // 最后写：数据驱动的头不得覆盖内容解释相关的头（防存储型 XSS）
    apply_file_security_headers(&mut headers, &content_type);
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
    session: OptionalSession,
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
    authorize_file_access(&file, session.0.as_ref())?;
    let total = storage.stat(&params.name).await?.content_length();

    let mut resp_headers = header::HeaderMap::with_capacity(8);
    // 声明支持 Range，客户端据此发起分片请求
    resp_headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Some(metadata) = file.get_metadata() {
        resp_headers.extend(metadata);
    }
    if let Some(response_headers) = ConfigurationModel::new()
        .get_response_headers(pool, &file.group)
        .await?
    {
        resp_headers.extend(response_headers);
    }
    // 最后写：数据驱动的头不得覆盖内容解释相关的头（防存储型 XSS）
    apply_file_security_headers(&mut resp_headers, &file.content_type);

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
    let ext = normalize_extension(&params.filename)?;
    let key = format!("{}.{}", uuid(), ext);
    let result = storage.presign_write(&key, PRESIGN_EXPIRE).await?;
    Ok(Json(presign_resp(key, result)))
}

/// 校验预签名确认键必须符合 `{uuid}.{ext}` 形状（避免任意已有对象被冒领）。
fn is_presigned_uuid_key(key: &str) -> bool {
    let Some((stem, ext)) = key.split_once('.') else {
        return false;
    };
    let is_uuid = stem.len() == 36
        && stem.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        });
    is_uuid
        && !ext.is_empty()
        && ext.len() <= MAX_EXTENSION_LEN
        && ext.bytes().all(|b| b.is_ascii_alphanumeric())
}

#[derive(Debug, Deserialize, Clone, Validate, ToSchema)]
struct PresignConfirmParams {
    /// 文件所属分组（与上传时声明一致）
    #[validate(custom(function = "x_file_group"))]
    group: String,
    /// `GET /files/presign/upload` 返回的对象键（`{uuid}.{ext}`）
    #[validate(custom(function = "x_file_name"))]
    key: String,
}

#[derive(Debug, Serialize, ToSchema)]
struct PresignConfirmResp {
    /// 文件表记录 ID
    id: u64,
    /// 已登记对象键
    key: String,
    /// 通过 storage.stat 读取到的真实字节大小
    file_size: i64,
    /// 文件 MIME 类型
    content_type: String,
}

/// `POST /files/presign/confirm` —— 确认直传完成并将对象元数据登记入库。
#[utoipa::path(
    post,
    path = "/files/presign/confirm",
    tag = "file",
    request_body = PresignConfirmParams,
    responses(
        (status = 200, description = "登记成功", body = PresignConfirmResp),
        (status = 400, description = "对象键不合法"),
        (status = 401, description = "未登录"),
        (status = 404, description = "对象尚未上传到存储"),
        (status = 409, description = "对象已登记")
    )
)]
async fn presign_confirm(
    State((storage, pool)): State<(&'static Storage, &'static PgPool)>,
    session: UserSession,
    JsonParams(params): JsonParams<PresignConfirmParams>,
) -> JsonResult<PresignConfirmResp> {
    if !is_presigned_uuid_key(&params.key) {
        return Err(Error::InvalidPresignKey {
            key: params.key.clone(),
        }
        .into());
    }
    let ext = normalize_extension(&params.key)?;
    if FileModel::new()
        .get_by_name(pool, &params.key)
        .await?
        .is_some()
    {
        return Err(Error::FileAlreadyExists {
            name: params.key.clone(),
        }
        .into());
    }

    let meta = storage.stat(&params.key).await?;
    let file_size_u64 = meta.content_length();
    if file_size_u64 > MAX_UPLOAD_BYTES as u64 {
        return Err(Error::PayloadTooLarge {
            max: MAX_UPLOAD_BYTES,
        }
        .into());
    }
    let file_size = i64::try_from(file_size_u64).unwrap_or(MAX_UPLOAD_BYTES as i64);
    let content_type = meta
        .content_type()
        .filter(|ct| !ct.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            mime_guess2::from_ext(&ext)
                .first_or_octet_stream()
                .essence_str()
                .to_string()
        });

    let id = FileModel::new()
        .insert_file(
            pool,
            FileInsertParams {
                group: params.group,
                filename: params.key.clone(),
                file_size,
                content_type: content_type.clone(),
                uploader: session.get_account().to_string(),
                ..Default::default()
            },
        )
        .await?;

    Ok(Json(PresignConfirmResp {
        id,
        key: params.key,
        file_size,
        content_type,
    }))
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
        (status = 403, description = "无权访问私有分组文件"),
        (status = 404, description = "对象不存在")
    )
)]
async fn presign_download(
    State((storage, pool)): State<(&'static Storage, &'static PgPool)>,
    session: UserSession,
    QueryParams(params): QueryParams<PresignDownloadParams>,
) -> JsonResult<PresignResp> {
    // 仅对已登记对象签名：未登记直接 404，避免被用来探测存储桶
    let file = FileModel::new()
        .get_by_name(pool, &params.name)
        .await?
        .context(FileNotFoundSnafu {
            name: params.name.clone(),
        })?;
    authorize_file_access(&file, Some(&*session))?;
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
            // 上传走 multipart 流式，关闭全局请求体上限（由 create_file 内部按 MAX_UPLOAD_BYTES
            // 逐块校验并在超限时中止+413），避免被全局 DefaultBodyLimit 在 handler 之前截断
            post(create_file)
                .layer(DefaultBodyLimit::disable())
                .with_state((params.storage, params.pool)),
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
            "/presign/confirm",
            post(presign_confirm).with_state((params.storage, params.pool)),
        )
        .route(
            "/presign/download",
            get(presign_download).with_state((params.storage, params.pool)),
        )
}

/// 本路由模块的 OpenAPI 文档片段（路径相对 `/files` 已在注解里写全）。
#[derive(OpenApi)]
#[openapi(
    paths(create_file, get_file, download_file, presign_upload, presign_confirm, presign_download),
    components(schemas(PresignResp, PresignConfirmParams, PresignConfirmResp)),
    tags((name = "file", description = "文件上传与预览"))
)]
struct FileApiDoc;

/// 返回 file 路由的 OpenAPI 文档片段，供主 crate 合并进全局文档。
pub fn openapi() -> utoipa::openapi::OpenApi {
    FileApiDoc::openapi()
}

#[cfg(test)]
mod tests {
    use super::{
        HeaderValue, RangeSpec, apply_file_security_headers, content_disposition_for, header,
        parse_byte_range,
    };

    /// **回归守卫**：SVG 可内嵌脚本，不得 inline；栅格图片仍可 inline。
    #[test]
    fn only_raster_images_are_inline() {
        for ct in ["image/png", "IMAGE/JPEG", "image/webp; charset=binary"] {
            assert_eq!(content_disposition_for(ct), "inline", "{ct}");
        }
        for ct in ["image/svg+xml", "text/html", "application/xhtml+xml", ""] {
            assert_eq!(content_disposition_for(ct), "attachment", "{ct}");
        }
    }

    /// 数据驱动的头先写入，安全头最后覆盖：`Content-Disposition: inline` 等不得生效。
    #[test]
    fn security_headers_override_data_headers() {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("inline"),
        );
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
        apply_file_security_headers(&mut headers, "text/html");
        assert_eq!(headers[header::CONTENT_TYPE], "text/html");
        assert_eq!(headers[header::CONTENT_DISPOSITION], "attachment");
        assert_eq!(headers[header::CONTENT_SECURITY_POLICY], "sandbox");
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    }

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
        assert!(matches!(
            parse_byte_range("items=0-1", 1000),
            RangeSpec::Full
        ));
        // 多段不支持，退化整文件
        assert!(matches!(
            parse_byte_range("bytes=0-1,2-3", 1000),
            RangeSpec::Full
        ));
        // 非数字
        assert!(matches!(
            parse_byte_range("bytes=a-b", 1000),
            RangeSpec::Full
        ));
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

    #[test]
    fn extension_validation_and_presign_key_format() {
        assert_eq!(
            super::normalize_extension("Photo.PNG").expect("valid ext"),
            "png"
        );
        assert!(super::normalize_extension("noext").is_err());
        assert!(super::normalize_extension("bad.p$h").is_err());
        assert!(super::normalize_extension(&format!("a.{}", "x".repeat(17))).is_err());

        assert!(super::is_presigned_uuid_key(
            "01936b8c-1234-7000-8000-000000000001.webp"
        ));
        assert!(!super::is_presigned_uuid_key("arbitrary.webp"));
        assert!(!super::is_presigned_uuid_key(
            "dir/01936b8c-1234-7000-8000-000000000001.webp"
        ));
    }

    #[test]
    fn error_mapping_and_raster_detection() {
        let img_err = super::BaseError::from(super::Error::ImageDimensions {
            message: "too wide".to_string(),
        });
        assert_eq!(img_err.status(), 400);
        assert!(!img_err.is_exception());

        let forbidden = super::BaseError::from(super::Error::Forbidden);
        assert_eq!(forbidden.status(), 403);

        assert!(super::is_raster_image("image/png"));
        assert!(!super::is_raster_image("image/svg+xml"));
        assert!(!super::is_raster_image("application/pdf"));
    }
}
