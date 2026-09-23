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

//! 登录凭证撤销标记：按用户 / 按角色记录「撤销时刻」。
//!
//! Session 与 JWT access token 都在签发时缓存了 roles / permissions，签发后
//! 服务端不再回查数据库。禁用账号、收回角色或权限、重置密码之后，已签发的
//! 凭证仍会按旧身份工作直到过期。
//!
//! 这里不去枚举并删除凭证（Redis 中没有「用户 → 凭证」索引，JWT 更是无状态），
//! 而是写入撤销时刻：凡签发时间 `iat` **早于**撤销时刻的凭证一律视为失效。
//! 校验只需一次 MGET（用户标记 + 各角色标记），会话与 JWT 共用同一套键。
//!
//! 标记的 TTL 应不短于受影响凭证的最长有效期——过了这个时间，被它针对的旧
//! 凭证本身也已过期，标记可以安全消失。

use super::{Error, RedisCache};
use std::time::Duration;
use tibba_util::timestamp;

type Result<T> = std::result::Result<T, Error>;

fn user_key(user_id: i64) -> String {
    format!("revoke:user:{user_id}")
}

fn role_key(role: &str) -> String {
    format!("revoke:role:{role}")
}

/// 使该用户此前签发的所有凭证失效。
pub async fn revoke_user_credentials(
    cache: &RedisCache,
    user_id: i64,
    ttl: Duration,
) -> Result<()> {
    cache.set(&user_key(user_id), timestamp(), Some(ttl)).await
}

/// 使持有该角色的用户此前签发的所有凭证失效（角色的权限被收回时使用）。
pub async fn revoke_role_credentials(cache: &RedisCache, role: &str, ttl: Duration) -> Result<()> {
    cache.set(&role_key(role), timestamp(), Some(ttl)).await
}

/// 签发于 `iat` 的凭证是否已被撤销（按用户或按其任一角色）。
pub async fn is_credential_revoked(
    cache: &RedisCache,
    user_id: i64,
    roles: &[String],
    iat: i64,
) -> Result<bool> {
    let mut keys = Vec::with_capacity(roles.len() + 1);
    keys.push(user_key(user_id));
    keys.extend(roles.iter().map(|role| role_key(role)));
    let marks: Vec<Option<i64>> = cache.mget(&keys).await?;
    Ok(revoked_since(iat, &marks))
}

/// 凭证签发时间是否早于任一撤销时刻。
///
/// 同一秒内「撤销后立即重新登录」签发的新凭证 `iat == 撤销时刻`，不应被误杀，
/// 故用严格小于。
fn revoked_since(iat: i64, marks: &[Option<i64>]) -> bool {
    marks.iter().flatten().any(|&revoked_at| iat < revoked_at)
}

#[cfg(test)]
mod tests {
    use super::revoked_since;

    #[test]
    fn credentials_issued_before_any_mark_are_revoked() {
        assert!(!revoked_since(100, &[]));
        assert!(!revoked_since(100, &[None, None]));
        // 用户标记
        assert!(revoked_since(99, &[Some(100), None]));
        // 任一角色标记
        assert!(revoked_since(99, &[None, None, Some(100)]));
        // 撤销后（含同一秒）重新签发：有效
        assert!(!revoked_since(100, &[Some(100)]));
        assert!(!revoked_since(101, &[Some(50), Some(100)]));
    }
}
