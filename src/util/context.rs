use axum::http::Extensions;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub trace_id: String,
    pub account: String,
}

impl Context {
    pub fn new() -> Self {
        Context::default()
    }
}

pub fn set_context(exts: &mut Extensions, ctx: Context) -> Option<Context> {
    exts.insert(ctx)
}

pub fn get_context(exts: &Extensions) -> Context {
    if let Some(ctx) = exts.get::<Context>() {
        return ctx.clone();
    }
    Context::new()
}
