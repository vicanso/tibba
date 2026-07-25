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

//! 进程级状态：`AppState`（并发计数 / 版本信息）、task-local 请求上下文 `CTX`，
//! 以及进程资源采样（`process-info` feature）。

mod app_state;
mod ctx;
// 进程指标采样依赖 sysinfo（传递依赖较重），默认不编译；
// 只用 AppState / CTX 的下游（middleware、session）因此无需为其付出编译代价。
#[cfg(feature = "process-info")]
mod process;

pub use app_state::*;
pub use ctx::*;
#[cfg(feature = "process-info")]
pub use process::*;
