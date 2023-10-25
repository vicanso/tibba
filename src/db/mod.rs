use crate::config::must_new_database_config;
use regex::Regex;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use tokio::sync::OnceCell;
use tracing::info;

async fn get_conn() -> DatabaseConnection {
    let config = must_new_database_config();
    let mut opt = ConnectOptions::new(&config.url);
    opt.max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .connect_timeout(config.connect_timeout)
        .acquire_timeout(config.acquire_timeout)
        .idle_timeout(config.idle_timeout);
    // .sqlx_logging(true);

    // opt.sqlx_logging(false) // Disabling SQLx log
    // .sqlx_logging_level(log::LevelFilter::Info);
    let re = Regex::new(r"\:\S+?@").unwrap();
    let url = re.replace(&config.origin_url, ":***@");
    let db = Database::connect(opt).await.unwrap();
    info!(url = url.to_string(), "connect to database success");

    db
}

pub async fn get_database() -> &'static DatabaseConnection {
    static DB: OnceCell<DatabaseConnection> = OnceCell::const_new();
    DB.get_or_init(get_conn).await
}
