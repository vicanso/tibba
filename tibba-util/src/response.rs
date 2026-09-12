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
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use std::fmt::Write;
use std::time::Duration;
use tibba_error::Error;

type Result<T> = std::result::Result<T, Error>;

pub type JsonResult<T> = Result<Json<T>>;

/// 共享缓存（CDN / 反向代理）的 `s-maxage` 上限。
///
/// 浏览器私有缓存可以按调用方给的时长缓存，但共享缓存的失效代价高得多
/// （要挨个节点刷），故统一压到 1 小时以内。
const S_MAX_AGE_LIMIT_SECS: u64 = 3600;

/// 带 `Cache-Control` 的 JSON 响应。
///
/// 默认 **`private`**——只允许终端浏览器缓存，不进 CDN / 反向代理等共享缓存。
///
/// # 为什么默认 private
/// 本类型最常见的用法是给「读多写少」的接口加一层缓存，而这类接口往往同时
/// 是**要鉴权**的。`public` 会让 CDN 把 A 用户的响应原样发给 B 用户——一次
/// 跨账号的数据泄漏，而且因为命中的是缓存，服务端日志里什么都看不到。
///
/// 确认响应与调用者身份无关（公开配置、字典表、版本号等）时，显式
/// [`Self::with_public`] 打开共享缓存。把「是否公开」交给调用方声明一次，
/// 好过默认公开、指望每个调用点都记得关。
///
/// 字段私有，按项目约定通过 [`Self::new`] + 链式方法配置。
pub struct CacheJson<T> {
    /// 缓存时长；为 0 时改发 `no-cache`
    duration: Duration,
    /// 是否允许共享缓存（CDN / 反向代理）存储
    public: bool,
    /// 响应数据
    data: T,
}

pub type CacheJsonResult<T> = Result<CacheJson<T>>;

impl<T> CacheJson<T> {
    /// 以缓存时长与数据创建响应，默认 `private`。
    #[must_use]
    pub fn new(duration: Duration, data: T) -> Self {
        Self {
            duration,
            public: false,
            data,
        }
    }

    /// 允许共享缓存（CDN / 反向代理）存储本响应，支持链式调用。
    ///
    /// **仅当响应内容与调用者身份完全无关时才可使用**，见类型文档。
    #[must_use]
    pub fn with_public(mut self) -> Self {
        self.public = true;
        self
    }
}

/// `(duration, data)` 的便捷转换，语义同 [`CacheJson::new`]（即默认 private）。
impl<T> From<(Duration, T)> for CacheJson<T> {
    fn from(value: (Duration, T)) -> Self {
        Self::new(value.0, value.1)
    }
}

impl<T> IntoResponse for CacheJson<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        let secs = self.duration.as_secs();

        // 时长为 0 等于「不要缓存」。发 `max-age=0` 在部分实现里仍允许带
        // 校验的复用，语义不如 `no-cache` 明确。
        if secs == 0 {
            return (
                [(header::CACHE_CONTROL, "no-cache".to_string())],
                Json(self.data),
            )
                .into_response();
        }

        // 用 `write!` 直接写进 String，比 format! + push_str 少若干次分配；
        // 预留容量覆盖最长形态。写 String 不会失败，故忽略返回值。
        let mut cache_control_value = String::with_capacity(64);
        let visibility = if self.public { "public" } else { "private" };
        let _ = write!(&mut cache_control_value, "{visibility}, max-age={secs}");

        // s-maxage 只对共享缓存有意义，private 时不必输出
        if self.public && secs > S_MAX_AGE_LIMIT_SECS {
            let _ = write!(
                &mut cache_control_value,
                ", s-maxage={S_MAX_AGE_LIMIT_SECS}"
            );
        }

        (
            [(header::CACHE_CONTROL, cache_control_value)],
            Json(self.data),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn cache_control<T: Serialize>(value: CacheJson<T>) -> String {
        value
            .into_response()
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    }

    /// **默认必须是 private**：本类型常挂在要鉴权的接口上，
    /// 默认 public 会让 CDN 把 A 用户的响应发给 B 用户。
    #[test]
    fn defaults_to_private() {
        let header = cache_control(CacheJson::new(Duration::from_secs(60), "x"));
        assert_eq!(header, "private, max-age=60");
        // From 转换走同一条默认
        let header = cache_control(CacheJson::from((Duration::from_secs(60), "x")));
        assert_eq!(header, "private, max-age=60");
    }

    #[test]
    fn public_opt_in_caps_shared_cache_ttl() {
        // 未超上限时不加 s-maxage
        let header = cache_control(CacheJson::new(Duration::from_secs(60), "x").with_public());
        assert_eq!(header, "public, max-age=60");

        // 超过上限时压住共享缓存
        let header = cache_control(CacheJson::new(Duration::from_secs(7200), "x").with_public());
        assert_eq!(header, "public, max-age=7200, s-maxage=3600");
    }

    /// private 响应不该出现 s-maxage —— 它只对共享缓存有意义。
    #[test]
    fn private_never_emits_s_maxage() {
        let header = cache_control(CacheJson::new(Duration::from_secs(7200), "x"));
        assert_eq!(header, "private, max-age=7200");
    }

    #[test]
    fn zero_duration_means_no_cache() {
        assert_eq!(cache_control(CacheJson::new(Duration::ZERO, "x")), "no-cache");
    }
}
