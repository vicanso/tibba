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

//! 应用级共享依赖容器（显式 DI 入口）。
//!
//! ## 动机
//! 历史上 DB / Redis / OpenDAL / AppState 各自 `OnceCell` + `get_*()`，业务与启动代码
//! 隐式依赖全局，单测难 mock、依赖图不透明。
//!
//! ## 用法
//! 1. `run_before_tasks` 完成后调用 [`AppCtx::install_from_globals`] 组装一次；
//! 2. 启动路径与新代码通过 [`get_app_ctx`] 或函数参数拿到引用；
//! 3. 底层 `get_db_pool` / `get_redis_cache` 等**兼容层仍可用**，逐步迁移即可。
//!
//! AppCtx **只聚合 `'static` 引用**，不拥有连接池本身；资源生命周期仍由各模块 hook 管理。

use crate::cache::get_redis_cache;
use crate::dal::get_opendal_storage;
use crate::sql::get_db_pool;
use crate::state::get_app_state;
use sqlx::PgPool;
use std::sync::OnceLock;
use tibba_cache::RedisCache;
use tibba_error::Error;
use tibba_opendal::Storage;
use tibba_state::AppState;

type Result<T, E = Error> = std::result::Result<T, E>;

static APP_CTX: OnceLock<AppCtx> = OnceLock::new();

/// 应用级共享依赖：进程内单例，在 hook before 全部就绪后安装。
#[derive(Clone, Copy)]
pub struct AppCtx {
    /// 运行态（并发上限、版本、running 标志等）
    pub state: &'static AppState,
    /// PostgreSQL 连接池
    pub pool: &'static PgPool,
    /// Redis 缓存封装
    pub cache: &'static RedisCache,
    /// 对象存储（OpenDAL）
    pub storage: &'static Storage,
}

impl AppCtx {
    /// 从已初始化的全局 `get_*` 组装一份上下文（不写入进程单例）。
    ///
    /// 要求 sql / redis / dal / state 的 before hook 已成功执行，否则会 panic。
    /// 测试或自定义启动可直接构造 [`AppCtx`] 字面量后调用 [`AppCtx::install`]。
    #[must_use]
    pub fn from_globals() -> Self {
        Self {
            state: get_app_state(),
            pool: get_db_pool(),
            cache: get_redis_cache(),
            storage: get_opendal_storage(),
        }
    }

    /// 安装为进程级单例。重复调用返回错误（fail-fast，避免静默覆盖）。
    pub fn install(self) -> Result<&'static AppCtx> {
        APP_CTX
            .set(self)
            .map_err(|_| Error::new("app ctx already installed").with_category("app_ctx"))?;
        Ok(get_app_ctx())
    }

    /// 从全局资源组装并安装；`run_before_tasks` 成功后调用一次。
    pub fn install_from_globals() -> Result<&'static AppCtx> {
        Self::from_globals().install()
    }
}

/// 返回已安装的应用上下文。未安装时 panic（与 `get_db_pool` 等同级契约）。
#[allow(dead_code)] // 供业务模块按需从全局取 ctx；启动路径经 `install_from_globals` 返回值使用
pub fn get_app_ctx() -> &'static AppCtx {
    APP_CTX.get().unwrap_or_else(|| {
        panic!("app ctx not installed; call AppCtx::install_from_globals after run_before_tasks")
    })
}
