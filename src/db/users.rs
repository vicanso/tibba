use super::{get_database, FindRecordParams};
use crate::entities::users;
use crate::error::HttpError;
use sea_orm::{entity::prelude::*, ActiveValue, QuerySelect};
use serde_json::Value;

pub type Result<T, E = HttpError> = std::result::Result<T, E>;

pub async fn add_user(account: &str, password: &str) -> Result<users::Model> {
    let conn = get_database().await;
    let result = users::ActiveModel {
        account: ActiveValue::set(account.to_string()),
        password: ActiveValue::set(password.to_string()),
        ..Default::default()
    }
    .insert(conn)
    .await?;
    Ok(result)
}

pub async fn find_user_by_account(account: &str) -> Result<Option<users::Model>> {
    let result = users::Entity::find()
        .filter(users::Column::Account.eq(account))
        .one(get_database().await)
        .await?;
    Ok(result)
}

pub async fn find_count_user_json(params: FindRecordParams) -> Result<(i64, Vec<Value>)> {
    let conn = get_database().await;
    let page_count = if params.page == 0 {
        let count = users::Entity::find().count(conn).await?;
        let mut page_count = count / params.page_size;
        if count % params.page_size != 0 {
            page_count += 1;
        }
        page_count as i64
    } else {
        -1
    };

    let sql = users::Entity::find();
    let items = sql
        .select_only()
        .column(users::Column::Account)
        .column(users::Column::CreatedAt)
        .column(users::Column::Email)
        .column(users::Column::Groups)
        .column(users::Column::Id)
        .column(users::Column::Remark)
        .column(users::Column::Roles)
        .column(users::Column::Status)
        .column(users::Column::UpdatedAt)
        .into_json()
        .paginate(conn, params.page_size)
        .fetch_page(params.page)
        .await?;

    Ok((page_count, items))
}
