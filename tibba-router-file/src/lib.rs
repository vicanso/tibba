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
use axum::http::HeaderValue;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use imageoptimize::{ImageProcessingError, ProcessImage, run_with_image};
use serde::Deserialize;
use snafu::{OptionExt, ResultExt, Snafu};
use sqlx::PgPool;
use std::collections::HashMap;
use std::path::Path;
use tibba_error::Error as BaseError;
use tibba_model_builtin::{ConfigurationModel, FileInsertParams, FileModel, Model};
use tibba_opendal::Storage;
use tibba_session::UserSession;
use tibba_util::{JsonResult, QueryParams, uuid};
use tibba_validator::{x_file_group, x_file_name, x_image_format, x_image_quality};
use utoipa::{IntoParams, OpenApi};
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
    while let Some(field) = multipart.next_field().await.context(MultipartSnafu)? {
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

        let data = field.bytes().await.context(MultipartSnafu)?;
        let content_type = mime_guess2::from_path(&file_name).first_or_octet_stream();
        let mut params = FileInsertParams {
            group: create_file_params.group.clone(),
            filename: file.clone(),
            file_size: data.len() as i64,
            content_type: content_type.to_string(),
            uploader: session.get_account().to_string(),
            ..Default::default()
        };

        if content_type.type_() == "image" {
            let format = image::guess_format(&data).context(ImageSnafu)?;
            params.content_type = format.to_mime_type().to_string();
            let image = image::load_from_memory_with_format(&data, format).context(ImageSnafu)?;
            params.width = Some(image.width() as i32);
            params.height = Some(image.height() as i32);
        };

        storage.write_with(&file, data.clone(), vec![]).await?;
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
}

/// 本路由模块的 OpenAPI 文档片段（路径相对 `/files` 已在注解里写全）。
#[derive(OpenApi)]
#[openapi(
    paths(create_file, get_file),
    tags((name = "file", description = "文件上传与预览"))
)]
struct FileApiDoc;

/// 返回 file 路由的 OpenAPI 文档片段，供主 crate 合并进全局文档。
pub fn openapi() -> utoipa::openapi::OpenApi {
    FileApiDoc::openapi()
}
