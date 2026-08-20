pub mod chart;
pub mod nav;
pub mod price_table;

use leptos::prelude::ServerFnError;

/// Pulls a displayable message out of a finished server action.
///
/// Server functions here return `ServerFnError::ServerError` with a message already
/// written for a person to read, so it is shown as-is.
pub fn action_error<T>(value: Option<Result<T, ServerFnError>>) -> Option<String> {
    match value {
        Some(Err(ServerFnError::ServerError(message))) => Some(message),
        Some(Err(other)) => Some(other.to_string()),
        _ => None,
    }
}
