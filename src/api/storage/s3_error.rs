use super::s3_xml::xml_escape;
use super::*;

pub(in crate::api::storage) fn s3_error_response(
    status: StatusCode,
    code: &str,
    message: &str,
    resource: &str,
    key: Option<&str>,
) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    let mut body = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    body.push_str("<Error>");
    body.push_str(&format!("<Code>{}</Code>", xml_escape(code)));
    body.push_str(&format!("<Message>{}</Message>", xml_escape(message)));
    if let Some(key) = key {
        body.push_str(&format!("<Key>{}</Key>", xml_escape(key)));
    }
    body.push_str(&format!("<Resource>{}</Resource>", xml_escape(resource)));
    body.push_str(&format!(
        "<RequestId>{}</RequestId>",
        xml_escape(&request_id)
    ));
    body.push_str("</Error>");

    apply_s3_response_headers(Response::builder().status(status))
        .header(header::CONTENT_TYPE, "application/xml")
        .body(axum::body::Body::from(body))
        .unwrap()
}
