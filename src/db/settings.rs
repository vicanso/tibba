use super::{
    get_database, EntityDescription, EntityItemCategory, EntityItemDescription, Error,
    ListCountParams, Result, ROLE_SU,
};
use crate::entities::settings::{ActiveModel, Column, Entity, Model};
use crate::util::{json_get_date_time, json_get_i64, json_get_string};
use once_cell::sync::Lazy;
use sea_orm::query::{Order, Select};
use sea_orm::Condition;
use sea_orm::{entity::prelude::*, ActiveValue::Set, QueryOrder};
use serde_json::Value;
use substring::Substring;

fn update_from_value(model: &mut ActiveModel, value: &Value) -> Result<()> {
    if let Some(status) = json_get_i64(value, Column::Status.as_str())? {
        model.status = Set(status as i8);
    }
    if let Some(name) = json_get_string(value, Column::Name.as_str())? {
        model.name = Set(name);
    }
    if let Some(category) = json_get_string(value, Column::Category.as_str())? {
        model.category = Set(category);
    }
    if let Some(data) = json_get_string(value, Column::Data.as_str())? {
        model.data = Set(data);
    }
    if let Some(remark) = json_get_string(value, Column::Remark.as_str())? {
        model.remark = Set(remark);
    }
    if let Some(started_at) = json_get_date_time(value, Column::StartedAt.as_str())? {
        model.started_at = Set(started_at);
    }
    if let Some(ended_at) = json_get_date_time(value, Column::EndedAt.as_str())? {
        model.ended_at = Set(ended_at);
    }
    Ok(())
}

static SUPPORT_ORDERS: Lazy<Vec<Column>> = Lazy::new(|| {
    vec![
        Column::StartedAt,
        Column::EndedAt,
        Column::UpdatedAt,
        Column::Status,
    ]
});

fn order_by<E>(sql: Select<E>, orders: &str) -> Result<Select<E>>
where
    E: EntityTrait,
{
    if orders.is_empty() {
        return Ok(sql);
    }
    let mut s = sql;
    let support_orders: Vec<(String, Column)> = SUPPORT_ORDERS
        .iter()
        .map(|item| (item.to_string(), *item))
        .collect();

    for order in orders.split(',') {
        let mut order_type = Order::Asc;
        let key = if order.starts_with('-') {
            order_type = Order::Desc;
            order.substring(1, order.len())
        } else {
            order
        };
        let mut found = false;
        for (name, column) in support_orders.iter() {
            if name == key {
                found = true;
                s = s.order_by(*column, order_type.clone());
            }
        }
        if !found {
            return Err(Error::OrderNotSupport {
                order: order.to_string(),
            }
            .into());
        }
    }
    Ok(s)
}

pub struct SettingEntity {}

impl SettingEntity {
    pub async fn update_by_id(user: &str, id: i64, value: &Value) -> Result<()> {
        let conn = get_database().await;
        let result = Entity::find_by_id(id).one(conn).await?;
        if result.is_none() {
            return Err(Error::NotFound.into());
        }
        let mut setting: ActiveModel = result.unwrap().into();
        setting.updater = Set(Some(user.to_string()));
        update_from_value(&mut setting, value)?;
        setting.update(conn).await?;
        Ok(())
    }
    pub async fn insert(user: &str, value: &Value) -> Result<Model> {
        // TODO 权限校验
        let mut data = ActiveModel {
            ..Default::default()
        };
        update_from_value(&mut data, value)?;
        data.creator = Set(user.to_string());
        let result = data.insert(get_database().await).await?;
        Ok(result)
    }
    pub fn description() -> EntityDescription {
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
                name: Column::Name.to_string(),
                label: "名称".to_string(),
                category: EntityItemCategory::Text,
                readonly: true,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Status.to_string(),
                label: "状态".to_string(),
                width: Some(90),
                category: EntityItemCategory::Status,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Category.to_string(),
                label: "分类".to_string(),
                width: Some(80),
                category: EntityItemCategory::Text,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::StartedAt.to_string(),
                label: "生效时间".to_string(),
                width: Some(150),
                category: EntityItemCategory::DateTime,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::EndedAt.to_string(),
                label: "失效时间".to_string(),
                width: Some(150),
                category: EntityItemCategory::DateTime,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Creator.to_string(),
                label: "创建人".to_string(),
                width: Some(80),
                category: EntityItemCategory::Text,
                readonly: true,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Data.to_string(),
                label: "配置".to_string(),
                width: Some(200),
                category: EntityItemCategory::Editor,
                span: Some(3),
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::Remark.to_string(),
                label: "备注".to_string(),
                width: Some(120),
                span: Some(3),
                category: EntityItemCategory::Text,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::CreatedAt.to_string(),
                label: "创建时间".to_string(),
                width: Some(150),
                category: EntityItemCategory::DateTime,
                readonly: true,
                ..Default::default()
            },
            EntityItemDescription {
                name: Column::UpdatedAt.to_string(),
                label: "更新时间".to_string(),
                width: Some(150),
                category: EntityItemCategory::DateTime,
                readonly: true,
                ..Default::default()
            },
        ];
        EntityDescription {
            items,
            modify_roles: vec![ROLE_SU.to_string()],
            support_orders: SUPPORT_ORDERS.iter().map(|item| item.to_string()).collect(),
            ..Default::default()
        }
    }
    pub async fn find_by_id(_user: &str, id: i64) -> Result<Option<Value>> {
        let conn = get_database().await;
        let item = Entity::find_by_id(id).into_json().one(conn).await?;
        Ok(item)
    }
    pub async fn list_count(_user: &str, params: &ListCountParams) -> Result<(i64, Vec<Value>)> {
        // TODO 权限校验
        let conn = get_database().await;
        let mut sql = Entity::find();

        if let Some(keyword) = &params.keyword {
            let cond = Condition::any()
                .add(Column::Category.eq(keyword))
                .add(Column::Name.contains(keyword))
                .add(Column::Remark.contains(keyword));
            sql = sql.filter(cond);
        }

        let page_count = if params.counted {
            let count = sql.clone().count(conn).await?;
            let mut page_count = count / params.page_size;
            if count % params.page_size != 0 {
                page_count += 1;
            }
            page_count as i64
        } else {
            -1
        };

        sql = order_by(
            sql,
            &params.orders.clone().unwrap_or("-updated_at".to_string()),
        )?;
        let items = sql
            .into_json()
            .paginate(conn, params.page_size)
            .fetch_page(params.page)
            .await?;

        Ok((page_count, items))
    }
}
