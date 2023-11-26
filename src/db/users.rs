use super::{get_database, EntityItemCategory, EntityItemDescription, ListCountParams, Result};
use crate::entities::users::{ActiveModel, Column, Entity, Model};
use sea_orm::{entity::prelude::*, ActiveValue, Iterable, QuerySelect};
use serde_json::Value;

pub async fn add_user(account: &str, password: &str) -> Result<Model> {
    let conn = get_database().await;
    let result = ActiveModel {
        account: ActiveValue::set(account.to_string()),
        password: ActiveValue::set(password.to_string()),
        ..Default::default()
    }
    .insert(conn)
    .await?;
    Ok(result)
}

pub async fn find_user_by_account(account: &str) -> Result<Option<Model>> {
    let result = Entity::find()
        .filter(Column::Account.eq(account))
        .one(get_database().await)
        .await?;
    Ok(result)
}

pub struct UserEntity {}

impl UserEntity {
    pub fn list_descriptions() -> Vec<EntityItemDescription> {
        vec![
            EntityItemDescription {
                name: Column::Id.to_string(),
                label: "ID".to_string(),
                width: Some(60),
                category: EntityItemCategory::Number,
                readonly: true,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Status.to_string(),
                label: "状态".to_string(),
                width: Some(60),
                category: EntityItemCategory::Status,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Account.to_string(),
                label: "账号".to_string(),
                category: EntityItemCategory::Text,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Email.to_string(),
                label: "邮箱".to_string(),
                width: Some(80),
                category: EntityItemCategory::Text,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Roles.to_string(),
                label: "角色".to_string(),
                width: Some(100),
                category: EntityItemCategory::Json,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Groups.to_string(),
                label: "群组".to_string(),
                width: Some(100),
                category: EntityItemCategory::Editor,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::CreatedAt.to_string(),
                label: "创建时间".to_string(),
                width: Some(140),
                category: EntityItemCategory::DateTime,
                readonly: true,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::UpdatedAt.to_string(),
                label: "更新时间".to_string(),
                width: Some(140),
                category: EntityItemCategory::DateTime,
                readonly: true,
                ..Default::default()
            },
        ]
    }
    pub async fn list_count(params: &ListCountParams) -> Result<(i64, Vec<Value>)> {
        let conn = get_database().await;
        let mut sql = Entity::find();
        if let Some(keyword) = &params.keyword {
            sql = sql.filter(Column::Account.contains(keyword));
        }
        let page_count = if params.page == 0 {
            let count = sql.clone().count(conn).await?;
            let mut page_count = count / params.page_size;
            if count % params.page_size != 0 {
                page_count += 1;
            }
            page_count as i64
        } else {
            -1
        };

        for item in Column::iter() {
            if item.as_str() == Column::Password.as_str() {
                continue;
            }
            sql = sql.column(item);
        }
        let items = sql
            .into_json()
            .paginate(conn, params.page_size)
            .fetch_page(params.page)
            .await?;

        Ok((page_count, items))
    }
}
