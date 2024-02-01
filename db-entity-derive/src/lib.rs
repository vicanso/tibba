extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::DeriveInput;

#[proc_macro_derive(DbEntity)]
pub fn db_entity(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).unwrap();
    let id = ast.ident;
    let gen = quote! {
        impl #id {
            fn order_by<E>(sql: Select<E>, orders: &str) -> Result<Select<E>>
            where E: EntityTrait {
                if orders.is_empty() {
                    return Ok(sql);
                }
                let mut s = sql;
                let support_orders: Vec<(String, Column)> = Self::get_support_orders()
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
            pub async fn update_by_id(user: &str, id: i64, value: &Value) -> Result<()> {
                Self::validate_for_update(user).await?;
                let conn = get_database().await;
                let result = Entity::find_by_id(id).one(conn).await?;
                if result.is_none() {
                    return Err(Error::NotFound.into());
                }
                let mut data: ActiveModel = result.unwrap().into();
                Self::update_from_value(&mut data, value)?;
                data.update(conn).await?;
                Ok(())
            }
            pub async fn insert(user: &str, value: &Value) -> Result<Model> {
                Self::validate_for_insert(user).await?;
                let mut data = ActiveModel {
                    ..Default::default()
                };
                Self::update_from_value(&mut data, value)?;
                data.creator = Set(user.to_string());
                let result = data.insert(get_database().await).await?;
                Ok(result)
            }
            pub async fn find_by_id(user: &str, id: i64) -> Result<Option<Value>> {
                Self::validate_for_query(user).await?;
                let conn = get_database().await;
                let item = Entity::find_by_id(id).into_json().one(conn).await?;
                Ok(item)
            }
            pub async fn list_count(user: &str, params: &ListCountParams) -> Result<(i64, Vec<Value>)> {
                Self::validate_for_query(user).await?;
                let conn = get_database().await;
                let mut sql = Entity::find();
                if let Some(cond) = Self::get_condition(params) {
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

                sql = Self::order_by(
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
    };
    gen.into()
}
