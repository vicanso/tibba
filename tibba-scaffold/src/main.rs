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

use rust_embed::RustEmbed;
use std::fs;
use std::path::Path;

// 嵌入 configs/ 目录（排除构建产物）
#[derive(RustEmbed)]
#[folder = "../configs/"]
#[exclude = "commit_id.txt"]
struct ConfigTemplates;

// 嵌入 admin/ 目录（排除 node_modules 和 dist 构建产物，保留 package-lock.json 锁定依赖版本）
#[derive(RustEmbed)]
#[folder = "../admin/"]
#[exclude = "node_modules/**"]
#[exclude = "dist/**"]
struct WebTemplates;

// 嵌入 sql/ 目录（包含所有内置模型与 token 相关迁移：
// configurations、users、files、token_*（accounts/recharges/usages/keys/prices/llms）等）
#[derive(RustEmbed)]
#[folder = "../sql/"]
struct SqlTemplates;

// 通用 src 文件（所有项目均需要）
const APP_CTX_RS: &str = include_str!("../../src/app_ctx.rs");
const CACHE_RS: &str = include_str!("../../src/cache.rs");
const CONFIG_RS: &str = include_str!("../../src/config.rs");
const DAL_RS: &str = include_str!("../../src/dal.rs");
const SQL_RS: &str = include_str!("../../src/sql.rs");
const STATE_RS: &str = include_str!("../../src/state.rs");
const ADMIN_WEB_RS: &str = include_str!("../../src/admin_web.rs");

// 独立项目 Cargo.toml 模板（已解析 workspace 依赖、移除 workspace 配置）
const CARGO_TOML_TPL: &str = include_str!("../templates/Cargo.toml");

// main.rs / router 模板（精简：无 docker/httpstat/job 样板业务）
const MAIN_RS_TPL: &str = include_str!("../templates/main.rs");
const ROUTER_RS_TPL: &str = include_str!("../templates/router.rs");

// Makefile 模板（裁剪自 workspace 根 Makefile，移除 workspace 专属目标）
const MAKEFILE_TPL: &str = include_str!("../templates/Makefile");

// Dockerfile 与 entrypoint 模板（路径与二进制名通过 {{name}} 占位符替换）
const DOCKERFILE_TPL: &str = include_str!("../templates/Dockerfile");
const ENTRYPOINT_TPL: &str = include_str!("../templates/entrypoint.sh");

fn write_file(dir: &Path, relative: &str, content: &[u8]) -> std::io::Result<()> {
    let target = dir.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, content)
}

fn write_text(dir: &Path, relative: &str, content: &str) -> std::io::Result<()> {
    write_file(dir, relative, content.as_bytes())
}

// 写入并赋予可执行权限（仅 unix 下生效，windows 上 NTFS 无 +x 概念）
fn write_executable(dir: &Path, relative: &str, content: &str) -> std::io::Result<()> {
    write_text(dir, relative, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let target = dir.join(relative);
        let mut perms = fs::metadata(&target)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&target, perms)?;
    }
    Ok(())
}

fn generate(name: &str, dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;

    // 通用 src 文件，直接复制（含 AppCtx DI 入口）
    write_text(dir, "src/app_ctx.rs", APP_CTX_RS)?;
    write_text(dir, "src/cache.rs", CACHE_RS)?;
    write_text(dir, "src/config.rs", CONFIG_RS)?;
    write_text(dir, "src/dal.rs", DAL_RS)?;
    write_text(dir, "src/sql.rs", SQL_RS)?;
    write_text(dir, "src/admin_web.rs", ADMIN_WEB_RS)?;
    write_text(dir, "src/router.rs", ROUTER_RS_TPL)?;

    // state.rs 替换项目名称
    let state = STATE_RS.replace(
        r#".with_name("tibba")"#,
        &format!(r#".with_name("{name}")"#),
    );
    write_text(dir, "src/state.rs", &state)?;

    // main.rs 从模板生成，替换 tracing target 占位符
    let main = MAIN_RS_TPL
        .replace("{{NAME}}", name)
        .replace("{{NAME_UPPER}}", &name.to_uppercase());
    write_text(dir, "src/main.rs", &main)?;

    // configs/ 目录，直接复制
    for file in ConfigTemplates::iter() {
        let data = ConfigTemplates::get(&file).unwrap();
        write_file(dir, &format!("configs/{file}"), &data.data)?;
    }

    // admin/ 目录，直接复制（开发者需运行 npm install && npm run build）
    for file in WebTemplates::iter() {
        let data = WebTemplates::get(&file).unwrap();
        write_file(dir, &format!("admin/{file}"), &data.data)?;
    }

    // sql/ 目录（PostgreSQL 迁移），用于初始化数据库 schema
    for file in SqlTemplates::iter() {
        let data = SqlTemplates::get(&file).unwrap();
        write_file(dir, &format!("sql/{file}"), &data.data)?;
    }

    // Cargo.toml 替换占位符
    let cargo_toml = CARGO_TOML_TPL.replace("{{name}}", name);
    write_text(dir, "Cargo.toml", &cargo_toml)?;

    // Makefile 直接复制
    write_text(dir, "Makefile", MAKEFILE_TPL)?;

    // Dockerfile 与 entrypoint.sh 替换 {{name}} 占位符
    write_text(dir, "Dockerfile", &DOCKERFILE_TPL.replace("{{name}}", name))?;
    // entrypoint.sh 需要可执行权限，否则 docker ENTRYPOINT 调用会失败
    write_executable(
        dir,
        "entrypoint.sh",
        &ENTRYPOINT_TPL.replace("{{name}}", name),
    )?;

    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: tibba-scaffold <项目名> [输出目录]");
        std::process::exit(1);
    }
    let name = &args[1];
    // 第二个参数为可选的输出目录，默认在当前目录下以项目名创建
    let dir = if let Some(output) = args.get(2) {
        // 支持 ~ 展开
        let expanded = if let Some(rest) = output.strip_prefix("~/") {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{home}/{rest}")
        } else {
            output.clone()
        };
        Path::new(&expanded).join(name).to_path_buf()
    } else {
        Path::new(name).to_path_buf()
    };
    if let Err(e) = generate(name, &dir) {
        eprintln!("生成失败: {e}");
        std::process::exit(1);
    }
    println!("项目 '{name}' 已创建于 '{}'", dir.display());
    println!();
    println!("接下来:");
    println!("  cd {}", dir.display());
    println!("  # 编辑 Cargo.toml，删除不需要的依赖");
    println!("  # 初始化数据库（PostgreSQL）：psql -d <db> -f sql/pg/init.sql");
    println!("  cd admin && npm install && npm run build && cd ..");
    println!("  cargo run");
}
