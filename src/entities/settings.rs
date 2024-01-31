//! `SeaORM` Entity. Generated by sea-orm-codegen 0.12.12

use async_trait::async_trait;
use chrono::{Duration, Utc};
use sea_orm::entity::prelude::*;
use sea_orm::ActiveValue;
use serde::{Deserialize, Serialize};
use std::ops::Add;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "settings")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub status: i8,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    #[sea_orm(unique)]
    pub name: String,
    pub category: String,
    #[sea_orm(column_type = "custom(\"LONGTEXT\")")]
    pub data: String,
    pub remark: String,
    pub started_at: DateTimeUtc,
    pub ended_at: DateTimeUtc,
    pub creator: String,
    pub updater: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModel {
    fn validate(&self) -> Result<(), DbErr> {
        if self.name.is_not_set() {
            return Err(DbErr::Custom("Name is required".to_string()));
        }
        if self.category.is_not_set() {
            return Err(DbErr::Custom("Category is required".to_string()));
        }
        if self.data.is_not_set() {
            return Err(DbErr::Custom("Data is required".to_string()));
        }
        if self.remark.is_not_set() {
            return Err(DbErr::Custom("Remark is required".to_string()));
        }
        Ok(())
    }
}

#[async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C: ConnectionTrait>(
        mut self,
        _db: &C,
        insert: bool,
    ) -> Result<Self, DbErr> {
        if insert {
            self.validate()?;
            self.created_at = ActiveValue::set(Utc::now());
            if self.started_at.is_not_set() {
                self.started_at = ActiveValue::set(Utc::now());
            }
            if self.ended_at.is_not_set() {
                self.ended_at = ActiveValue::set(Utc::now().add(Duration::days(365 * 10)));
            }
        }
        self.updated_at = ActiveValue::set(Utc::now());
        Ok(self)
    }
    async fn before_delete<C: ConnectionTrait>(self, _db: &C) -> Result<Self, DbErr> {
        // 禁止删除数据
        Err(DbErr::Custom("Delete is forbidden".to_string()))
    }
}
