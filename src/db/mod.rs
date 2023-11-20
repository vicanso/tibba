use serde::Deserialize;

mod conn;
mod settings;
mod users;

#[derive(Debug, Deserialize)]
pub struct FindRecordParams {
    pub table: String,
    pub orders: Option<String>,
    pub page: u64,
    pub page_size: u64,
}

pub use conn::get_database;
pub use settings::*;
pub use users::*;
