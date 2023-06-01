// copy from async-redis-session

use async_session::{async_trait, serde_json, Result, Session, SessionStore};
use redis::{aio::Connection, AsyncCommands, Client, IntoConnectionInfo, RedisResult};

/// # RedisSessionStore
#[derive(Clone, Debug)]
pub struct RedisSessionStore {
    client: Client,
    prefix: Option<String>,
}

impl RedisSessionStore {
    /// creates a redis store from an existing [`redis::Client`]
    /// ```rust
    /// # use async_redis_session::RedisSessionStore;
    /// let client = redis::Client::open("redis://127.0.0.1").unwrap();
    /// let store = RedisSessionStore::from_client(client);
    /// ```
    pub fn from_client(client: Client) -> Self {
        Self {
            client,
            prefix: None,
        }
    }

    /// creates a redis store from a [`redis::IntoConnectionInfo`]
    /// such as a [`String`], [`&str`](str), or [`Url`](../url/struct.Url.html)
    /// ```rust
    /// # use async_redis_session::RedisSessionStore;
    /// let store = RedisSessionStore::new("redis://127.0.0.1").unwrap();
    /// ```
    pub fn new(connection_info: impl IntoConnectionInfo) -> RedisResult<Self> {
        Ok(Self::from_client(Client::open(connection_info)?))
    }

    /// sets a key prefix for this session store
    ///
    /// ```rust
    /// # use async_redis_session::RedisSessionStore;
    /// let store = RedisSessionStore::new("redis://127.0.0.1").unwrap()
    ///     .with_prefix("async-sessions/");
    /// ```
    /// ```rust
    /// # use async_redis_session::RedisSessionStore;
    /// let client = redis::Client::open("redis://127.0.0.1").unwrap();
    /// let store = RedisSessionStore::from_client(client)
    ///     .with_prefix("async-sessions/");
    /// ```
    pub fn with_prefix(mut self, prefix: impl AsRef<str>) -> Self {
        self.prefix = Some(prefix.as_ref().to_owned());
        self
    }

    async fn ids(&self) -> Result<Vec<String>> {
        Ok(self.connection().await?.keys(self.prefix_key("*")).await?)
    }

    /// returns the number of sessions in this store
    pub async fn count(&self) -> Result<usize> {
        if self.prefix.is_none() {
            let mut connection = self.connection().await?;
            Ok(redis::cmd("DBSIZE").query_async(&mut connection).await?)
        } else {
            Ok(self.ids().await?.len())
        }
    }

    #[cfg(test)]
    async fn ttl_for_session(&self, session: &Session) -> Result<usize> {
        Ok(self
            .connection()
            .await?
            .ttl(self.prefix_key(session.id()))
            .await?)
    }

    fn prefix_key(&self, key: impl AsRef<str>) -> String {
        if let Some(ref prefix) = self.prefix {
            format!("{}{}", prefix, key.as_ref())
        } else {
            key.as_ref().into()
        }
    }

    async fn connection(&self) -> RedisResult<Connection> {
        self.client.get_async_connection().await
    }
}

#[async_trait]
impl SessionStore for RedisSessionStore {
    async fn load_session(&self, cookie_value: String) -> Result<Option<Session>> {
        let id = Session::id_from_cookie_value(&cookie_value)?;
        let mut connection = self.connection().await?;
        let record: Option<String> = connection.get(self.prefix_key(id)).await?;
        match record {
            Some(value) => Ok(serde_json::from_str(&value)?),
            None => Ok(None),
        }
    }

    async fn store_session(&self, session: Session) -> Result<Option<String>> {
        let id = self.prefix_key(session.id());
        let string = serde_json::to_string(&session)?;

        let mut connection = self.connection().await?;

        match session.expires_in() {
            None => connection.set(id, string).await?,

            Some(expiry) => {
                connection
                    .set_ex(id, string, expiry.as_secs() as usize)
                    .await?
            }
        };

        Ok(session.into_cookie_value())
    }

    async fn destroy_session(&self, session: Session) -> Result {
        let mut connection = self.connection().await?;
        let key = self.prefix_key(session.id());
        connection.del(key).await?;
        Ok(())
    }

    async fn clear_store(&self) -> Result {
        let mut connection = self.connection().await?;

        if self.prefix.is_none() {
            let _: () = redis::cmd("FLUSHDB").query_async(&mut connection).await?;
        } else {
            let ids = self.ids().await?;
            if !ids.is_empty() {
                connection.del(ids).await?;
            }
        }
        Ok(())
    }
}
