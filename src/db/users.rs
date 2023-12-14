use super::{
    get_database, EntityDescription, EntityItemCategory, EntityItemDescription, EntityItemOption,
    Error, ListCountParams, Result, ROLE_ADMIN, ROLE_SU,
};
use crate::entities::users::{ActiveModel, Column, Entity, Model};
use crate::util::{json_get_i64, json_get_strings};
use sea_orm::{entity::prelude::*, ActiveValue::Set, Condition, Iterable, QuerySelect};
use serde_json::{json, Value};

pub async fn add_user(account: &str, password: &str) -> Result<Model> {
    let conn = get_database().await;
    let result = ActiveModel {
        account: Set(account.to_string()),
        password: Set(password.to_string()),
        ..Default::default()
    }
    .insert(conn)
    .await?;
    if result.id == 1 {
        let mut user: ActiveModel = result.clone().into();
        user.roles = Set(Some(json!([ROLE_SU])));
        // 仅输出日志即可
        _ = user.update(conn).await
    }
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
    pub async fn update_by_id(user: &str, id: i64, value: &Value) -> Result<()> {
        let conn = get_database().await;
        let result = Entity::find_by_id(id).one(conn).await?;
        if result.is_none() {
            return Err(Error::NotFound.into());
        }
        let mut data: ActiveModel = result.unwrap().into();
        if let Some(value) = json_get_i64(value, Column::Status.as_str())? {
            data.status = Set(value as i8);
        }
        if let Some(value) = json_get_strings(value, Column::Roles.as_str())? {
            data.roles = Set(Some(json!(value)))
        }
        if let Some(value) = json_get_strings(value, Column::Groups.as_str())? {
            data.groups = Set(Some(json!(value)))
        }
        data.update(conn).await?;
        Ok(())
    }
    pub fn description() -> EntityDescription {
        let roles = vec![ROLE_SU, ROLE_ADMIN];
        let role_options = roles
            .iter()
            .map(|item| EntityItemOption {
                label: item.to_string(),
                str_value: Some(item.to_string()),
                ..Default::default()
            })
            .collect();
        let items = vec![
            EntityItemDescription {
                name: Column::Id.to_string(),
                label: "ID".to_string(),
                width: Some(60),
                category: EntityItemCategory::Number,
                readonly: true,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Account.to_string(),
                label: "账号".to_string(),
                category: EntityItemCategory::Text,
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
                category: EntityItemCategory::TEXTS,
                options: Some(role_options),
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
        ];
        EntityDescription {
            items,
            modify_roles: vec![ROLE_SU.to_string()],
            ..Default::default()
        }
    }
    pub async fn find_by_id(_user: &str, id: i64) -> Result<Option<Value>> {
        let conn = get_database().await;
        let item = Entity::find_by_id(id)
            .select_only()
            .columns(Column::iter().filter(|col| !matches!(col, Column::Password)))
            .into_json()
            .one(conn)
            .await?;
        Ok(item)
    }
    pub async fn list_count(_user: &str, params: &ListCountParams) -> Result<(i64, Vec<Value>)> {
        // TODO 判断权限
        let conn = get_database().await;
        let mut sql = Entity::find();
        if let Some(keyword) = &params.keyword {
            let cond = Condition::any()
                .add(Column::Account.contains(keyword))
                .add(Column::Email.contains(keyword));
            sql = sql.filter(cond);
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

        let items = sql
            .select_only()
            .columns(Column::iter().filter(|col| !matches!(col, Column::Password)))
            .into_json()
            .paginate(conn, params.page_size)
            .fetch_page(params.page)
            .await?;

        Ok((page_count, items))
    }
}
