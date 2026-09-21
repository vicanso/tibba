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

//! 跨节点缓存失效广播（Redis pub/sub）。
//!
//! ## 解决什么
//! [`crate::TwoLevelStore`] 的 L1 在进程内。某个节点改了数据、清了自己的 L1，
//! **其余节点毫不知情**，会继续返回旧值直到各自的 L1 到期。
//!
//! 它原本靠「L1 TTL 对齐到墙钟边界」把分歧窗口收敛成「至多到下一个边界」——
//! 对特性开关这类容忍短暂陈旧的数据够用，但对「改完要立刻生效」的数据不够：
//! 权限变更、配置下发、账号封禁，等一个边界都太久。
//!
//! 本模块补上那条广播通道：写入方 `PUBLISH` 一条 key，各节点收到就清掉自己的
//! L1，下次读取自然回源。
//!
//! ## 刻意的设计取舍
//!
//! **广播的是「失效」而不是「新值」。** 发新值看起来更省一次回源，但那要求
//! 消息不丢、不乱序——pub/sub 两样都不保证。收到失效只是丢弃本地副本，
//! 丢一条消息的后果退化回原本的 TTL 行为；发新值一旦乱序，节点会把旧值当新值
//! 写进 L1 并一直留到过期，比不做还糟。
//!
//! **best-effort，不影响写入结果。** 广播失败只记日志：Redis 里的数据已经写对了，
//! 边界对齐仍在兜底。让一次 pub/sub 抖动把业务写入判为失败是本末倒置。
//!
//! **跳过自己发的消息。** 每条消息带发送方 node id。不跳的话，本节点刚写完就把
//! 自己刚填好的 L1 清掉，每次写入白白多一次回源。
//!
//! ## 可靠性边界
//! Redis pub/sub 是**至多一次**投递：订阅方断线期间的消息不会补发。因此本机制
//! 是「让失效更快」，不是「保证一致」——L1 的 TTL 仍然是那个兜底，不能去掉。

use super::{Error, RedisClient, RedisSnafu};
use futures::StreamExt;
use redis::{AsyncCommands, Client as RedisRawClient};
use snafu::ResultExt;
use std::sync::Arc;
use std::time::Duration;
use tibba_util::nanoid;
use tokio::sync::watch;
use tracing::{debug, info, warn};

use super::LOG_TARGET;

type Result<T> = std::result::Result<T, Error>;

/// 发送方标识的长度。消息体是 `node_id || key`，定长前缀使切分不存在歧义——
/// 用分隔符则要考虑 key 自身含该字符的情况。
const NODE_ID_LEN: usize = 12;

/// 重连退避的起步与上限。
const RECONNECT_MIN_BACKOFF: Duration = Duration::from_millis(200);
const RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(30);

/// 跨节点失效广播通道。
///
/// 由 [`crate::TwoLevelStore::with_invalidation`] 装配；也可单独用于自建的
/// 进程内缓存。
pub struct InvalidationBus {
    client: &'static RedisClient,
    /// pub/sub 频道名。不同缓存用不同频道，避免互相唤醒。
    channel: String,
    /// 本节点标识，用于跳过自己发出的消息。
    node_id: String,
}

impl std::fmt::Debug for InvalidationBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvalidationBus")
            .field("channel", &self.channel)
            .field("node_id", &self.node_id)
            .finish()
    }
}

impl InvalidationBus {
    /// 在给定频道上创建广播通道。
    ///
    /// `channel` 应当每种缓存一个（如 `"inval:feature"`、`"inval:perm"`）：
    /// 共用一个频道会让所有节点为每一次任意缓存的写入都被唤醒一遍。
    #[must_use]
    pub fn new(client: &'static RedisClient, channel: impl Into<String>) -> Self {
        Self {
            client,
            channel: channel.into(),
            node_id: nanoid(NODE_ID_LEN),
        }
    }

    /// 本节点标识。
    #[must_use]
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// 广播一条失效通知。
    ///
    /// 调用方通常不直接用它——[`crate::TwoLevelStore`] 会在 `set` / `del` 时自动发。
    pub async fn publish(&self, key: &str) -> Result<()> {
        let mut conn = self.client.conn().await?;
        let payload = format!("{}{key}", self.node_id);
        let _: i64 = conn
            .publish(&self.channel, payload)
            .await
            .context(RedisSnafu {
                category: "invalidation_publish",
            })?;
        Ok(())
    }

    /// 启动后台订阅任务，收到他节点的失效通知时调用 `on_invalidate`。
    ///
    /// 返回的 [`InvalidationListener`] **必须持有**：它一旦被 drop，订阅任务就会
    /// 收到停止信号并退出。把它放进进程级状态，或用
    /// `tibba_runtime::shutdown_token()` 在停机时显式 `stop()`。
    ///
    /// 连接断开会自动重连（指数退避，上限 [`RECONNECT_MAX_BACKOFF`]）。断线
    /// 期间的消息**不会补发**，见模块文档的可靠性边界。
    #[must_use]
    pub fn spawn_listener<F>(&self, on_invalidate: F) -> InvalidationListener
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let (stop_tx, stop_rx) = watch::channel(false);
        let nodes = self.client.node_urls().to_vec();
        let channel = self.channel.clone();
        let node_id = self.node_id.clone();
        let callback = Arc::new(on_invalidate);

        tokio::spawn(async move {
            listen_loop(nodes, channel, node_id, callback, stop_rx).await;
        });

        InvalidationListener { stop: stop_tx }
    }
}

/// 订阅任务的句柄。drop 即停止。
pub struct InvalidationListener {
    stop: watch::Sender<bool>,
}

impl InvalidationListener {
    /// 通知订阅任务退出。幂等。
    pub fn stop(&self) {
        // 接收端已消失（任务提前退出）也无所谓，忽略结果
        let _ = self.stop.send(true);
    }
}

impl Drop for InvalidationListener {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 订阅主循环：连接 → 收消息 → 断开则退避重连，直到收到停止信号。
async fn listen_loop<F>(
    nodes: Vec<String>,
    channel: String,
    node_id: String,
    on_invalidate: Arc<F>,
    mut stop_rx: watch::Receiver<bool>,
) where
    F: Fn(&str) + Send + Sync + 'static,
{
    if nodes.is_empty() {
        warn!(target: LOG_TARGET, channel, "no redis node configured; invalidation listener not started");
        return;
    }

    let mut backoff = RECONNECT_MIN_BACKOFF;
    let mut attempt = 0usize;

    while !*stop_rx.borrow() {
        // 轮换节点：固定用第一个的话，那个节点一挂订阅就再也起不来
        let url = &nodes[attempt % nodes.len()];
        attempt = attempt.wrapping_add(1);

        match subscribe_once(url, &channel, &node_id, &on_invalidate, &mut stop_rx).await {
            Ok(()) => {
                // 收到停止信号，正常退出
                break;
            }
            Err(err) => {
                warn!(
                    target: LOG_TARGET,
                    channel,
                    error = %err,
                    backoff_ms = backoff.as_millis(),
                    "invalidation subscription lost; reconnecting",
                );
            }
        }

        // 退避期间也要能被叫停，否则停机要多等一个退避周期
        tokio::select! {
            () = tokio::time::sleep(backoff) => {}
            _ = stop_rx.changed() => {}
        }
        backoff = (backoff * 2).min(RECONNECT_MAX_BACKOFF);
    }

    info!(target: LOG_TARGET, channel, "invalidation listener stopped");
}

/// 建立一次订阅并消费消息。
///
/// 返回 `Ok(())` 表示收到停止信号；`Err` 表示连接层出问题，由上层退避重连。
async fn subscribe_once<F>(
    url: &str,
    channel: &str,
    node_id: &str,
    on_invalidate: &Arc<F>,
    stop_rx: &mut watch::Receiver<bool>,
) -> Result<()>
where
    F: Fn(&str) + Send + Sync + 'static,
{
    // pub/sub 必须独占连接：订阅之后这条连接只能收推送，不能再跑普通命令，
    // 所以它不能进 deadpool
    let client = RedisRawClient::open(url).context(RedisSnafu {
        category: "invalidation_open",
    })?;
    let mut pubsub = client.get_async_pubsub().await.context(RedisSnafu {
        category: "invalidation_connect",
    })?;
    pubsub.subscribe(channel).await.context(RedisSnafu {
        category: "invalidation_subscribe",
    })?;

    info!(target: LOG_TARGET, channel, "invalidation listener subscribed");
    // 重连成功后退避应当重置，由调用方在下一轮循环感知——这里只负责消费
    let mut stream = pubsub.on_message();

    loop {
        tokio::select! {
            _ = stop_rx.changed() => return Ok(()),
            msg = stream.next() => {
                let Some(msg) = msg else {
                    // 流结束 = 连接断了，交给上层重连
                    return Err(Error::Redis {
                        category: "invalidation_stream".to_string(),
                        source: redis::RedisError::from((
                            redis::ErrorKind::Io,
                            "pubsub stream ended",
                        )),
                    });
                };
                let payload: String = match msg.get_payload() {
                    Ok(v) => v,
                    // 单条消息坏掉不该拖垮订阅
                    Err(e) => {
                        debug!(target: LOG_TARGET, channel, error = %e, "invalid invalidation payload");
                        continue;
                    }
                };
                if let Some(key) = parse_message(&payload, node_id) {
                    debug!(target: LOG_TARGET, channel, key, "invalidate from peer");
                    on_invalidate(key);
                }
            }
        }
    }
}

/// 解析消息体，返回需要失效的 key；自己发的消息返回 `None`。
///
/// 抽成自由函数以便单测覆盖，无需真实 Redis。
fn parse_message<'a>(payload: &'a str, self_node_id: &str) -> Option<&'a str> {
    // 长度不足以容纳 node id 的消息是脏数据（别的程序在用同一频道？），丢弃
    if !payload.is_char_boundary(NODE_ID_LEN) {
        return None;
    }
    let (sender, key) = payload.split_at(NODE_ID_LEN);
    if sender == self_node_id || key.is_empty() {
        // 自己发的：本地 L1 已经是最新的，再清一次只会白白多一次回源
        return None;
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const ME: &str = "aaaaaaaaaaaa"; // 12 字符，与 NODE_ID_LEN 一致
    const PEER: &str = "bbbbbbbbbbbb";

    #[test]
    fn node_id_constant_matches_test_fixtures() {
        assert_eq!(ME.len(), NODE_ID_LEN);
        assert_eq!(PEER.len(), NODE_ID_LEN);
    }

    #[test]
    fn peer_message_yields_the_key() {
        assert_eq!(
            parse_message(&format!("{PEER}feature_flags"), ME),
            Some("feature_flags")
        );
    }

    /// **关键**：自己发的消息必须跳过。
    ///
    /// 不跳的话，本节点每次写入都会把自己刚填好的 L1 清掉，
    /// 于是紧接着的读全部白跑一趟 Redis。
    #[test]
    fn own_message_is_skipped() {
        assert_eq!(parse_message(&format!("{ME}feature_flags"), ME), None);
    }

    /// key 里含分隔符类字符不能影响切分——定长前缀正是为此。
    #[test]
    fn keys_with_arbitrary_characters_survive() {
        for key in ["a:b:c", "with space", "冒号：中文", "{tag}key", ""] {
            let payload = format!("{PEER}{key}");
            let parsed = parse_message(&payload, ME);
            if key.is_empty() {
                assert_eq!(parsed, None, "空 key 无意义，应丢弃");
            } else {
                assert_eq!(parsed, Some(key));
            }
        }
    }

    /// 脏消息（别的程序在用同一频道）不得 panic，也不得触发失效。
    #[test]
    fn malformed_payload_is_discarded() {
        // 长度不足
        assert_eq!(parse_message("short", ME), None);
        assert_eq!(parse_message("", ME), None);
        // 第 12 字节恰好落在多字节字符中间——直接 split_at(12) 会 panic。
        // 1 字节 + 3 字节字符，边界在 0/1/4/7/10/13…，12 不是边界
        let payload = format!("a{}", "中".repeat(6));
        assert!(!payload.is_char_boundary(NODE_ID_LEN), "构造前提不成立");
        assert_eq!(parse_message(&payload, ME), None);
    }

    /// 退避必须有上界，否则一次长时间故障之后重连间隔会涨到荒谬的量级。
    #[test]
    fn backoff_is_bounded() {
        let mut backoff = RECONNECT_MIN_BACKOFF;
        for _ in 0..50 {
            backoff = (backoff * 2).min(RECONNECT_MAX_BACKOFF);
        }
        assert_eq!(backoff, RECONNECT_MAX_BACKOFF);
        assert!(RECONNECT_MIN_BACKOFF < RECONNECT_MAX_BACKOFF);
    }
}
