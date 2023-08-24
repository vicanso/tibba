// copy from async-redis-session
use super::get_redis_conn;
use async_session::{async_trait, serde_json, Result, Session, SessionStore};
use redis::AsyncCommands;

/// # RedisSessionStore
#[derive(Clone, Debug)]
pub struct RedisSessionStore {
    prefix: Option<String>,
}

impl RedisSessionStore {
    pub fn new(prefix: Option<String>) -> Self {
        RedisSessionStore { prefix }
    }
    fn prefix_key(&self, key: impl AsRef<str>) -> String {
        if let Some(ref prefix) = self.prefix {
            format!("{}{}", prefix, key.as_ref())
        } else {
            key.as_ref().into()
        }
    }
}

#[async_trait]
impl SessionStore for RedisSessionStore {
    async fn load_session(&self, cookie_value: String) -> Result<Option<Session>> {
        let id = Session::id_from_cookie_value(&cookie_value)?;
        let mut connection = get_redis_conn().await?;
        let record: Option<String> = connection.get(self.prefix_key(id)).await?;
        match record {
            Some(value) => Ok(serde_json::from_str(&value)?),
            None => Ok(None),
        }
    }

    async fn store_session(&self, session: Session) -> Result<Option<String>> {
        let id = self.prefix_key(session.id());
        let string = serde_json::to_string(&session)?;

        let mut connection = get_redis_conn().await?;

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
        let mut connection = get_redis_conn().await?;
        let key = self.prefix_key(session.id());
        connection.del(key).await?;
        Ok(())
    }

    async fn clear_store(&self) -> Result {
        // 清除不做任何操作
        Ok(())
    }
}
