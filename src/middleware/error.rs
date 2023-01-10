use axum::{
    body::{Body, BoxBody},
    http::Request,
    middleware::Next,
    response::Response,
};

use crate::util::read_http_body;

pub async fn error_handler(req: Request<Body>, next: Next<Body>) -> Response {
    let mut resp = next.run(req).await;
    // if resp.status().as_u16() < 400 {
    //     return resp;
    // }
    // let (parts, body) = resp.into_parts();
    // BoxBody::new(body);
    // if let Ok(data) = read_http_body(body).await {
    //     let v:BoxBody =  Body::from("{}").into();
    //     Response::from_parts(parts, )
    // } else {
    //     Response::from_parts(parts, BoxBody::new(Body::from("{}")))
    // }
    // res
    // res

    // if let Ok(data) = read_http_body(body).await {

    // }

    // Response::from_parts(parts, body)
    resp
}
