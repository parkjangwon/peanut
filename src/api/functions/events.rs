use super::*;

pub async fn stream_function_events(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<Claims>,
    Path(name): Path<String>,
) -> Response {
    if let Some(response) = require_admin(&claims) {
        return response;
    }

    if let Err(LoadFunctionError::NotFound) = load_function_by_name(&state.pool, &name).await {
        return json_error(StatusCode::NOT_FOUND, "function not found");
    }

    let stream =
        BroadcastStream::new(state.functions.event_sender.subscribe()).filter_map(move |message| {
            match message {
                Ok(event) if event.function_name == name => Some(Ok::<Event, Infallible>(
                    Event::default()
                        .event("function.invocation")
                        .json_data(event)
                        .unwrap_or_else(|_| Event::default().data("{}")),
                )),
                _ => None,
            }
        });

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
        .into_response()
}
