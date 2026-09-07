//! Turning API values into tool results.

use rmcp::model::{CallToolResult, ContentBlock};
use serde::Serialize;
use serde_json::{Map, Value, json};

/// How much one tool may return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputLimits {
    /// Rows a listing returns before it truncates.
    pub max_items: usize,
    /// Bytes a listing may reach before it sheds rows.
    pub max_bytes: usize,
}

impl Default for OutputLimits {
    fn default() -> Self {
        Self {
            max_items: 100,
            max_bytes: 100_000,
        }
    }
}

/// A successful result carrying one value.
pub fn json<T: Serialize>(value: &T) -> CallToolResult {
    match serde_json::to_value(value) {
        Ok(value) => CallToolResult::structured(value),
        Err(err) => unserializable(&err),
    }
}

/// A successful result carrying a sentence and nothing else.
///
/// The write endpoints answer with an empty body, so a caller that has just created
/// something has nothing to name it by. Saying it here is what a follow-up call needs.
pub fn confirmation(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text)])
}

/// A caller's own argument was wrong, so no request was worth making.
pub fn rejected(text: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(text)])
}

/// A listing, with enough of a header to tell a short answer from a truncated one.
///
/// Truncation is reported rather than silent, because a caller that reads 100 of 900
/// mailboxes and believes it has them all will draw a confident wrong conclusion.
pub fn page<T: Serialize>(items: &[T], limits: OutputLimits) -> CallToolResult {
    let total = items.len();
    let mut shown = total.min(limits.max_items);

    loop {
        let value = match envelope(items, total, shown) {
            Ok(value) => value,
            Err(err) => return unserializable(&err),
        };

        // Halving rather than stepping: a listing that overshoots by an order of magnitude
        // should not be re-serialized a hundred times to find that out.
        let over_budget = value.to_string().len() > limits.max_bytes;
        if shown == 0 || !over_budget {
            return CallToolResult::structured(value);
        }
        shown /= 2;
    }
}

fn envelope<T: Serialize>(
    items: &[T],
    total: usize,
    shown: usize,
) -> Result<Value, serde_json::Error> {
    let mut envelope = Map::new();
    envelope.insert("total".to_owned(), json!(total));
    envelope.insert("returned".to_owned(), json!(shown));
    envelope.insert("truncated".to_owned(), json!(shown < total));
    envelope.insert("items".to_owned(), serde_json::to_value(&items[..shown])?);
    Ok(Value::Object(envelope))
}

/// A value the API returned that cannot be put on the wire.
///
/// Reported rather than swallowed: it means the client and the server disagree about a
/// number, which is worth seeing rather than reading as an empty result.
fn unserializable(err: &serde_json::Error) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!(
        "MXroute returned a value this server could not encode as JSON: {err}"
    ))])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("item-{i}")).collect()
    }

    fn body(result: &CallToolResult) -> &Value {
        result.structured_content.as_ref().unwrap_or(&Value::Null)
    }

    #[test]
    fn a_short_listing_is_not_reported_as_truncated() {
        let result = page(&items(3), OutputLimits::default());
        assert_eq!(body(&result)["total"], json!(3));
        assert_eq!(body(&result)["returned"], json!(3));
        assert_eq!(body(&result)["truncated"], json!(false));
    }

    #[test]
    fn a_long_listing_keeps_the_total_it_did_not_return() {
        let limits = OutputLimits {
            max_items: 10,
            ..OutputLimits::default()
        };
        let result = page(&items(250), limits);
        assert_eq!(body(&result)["total"], json!(250));
        assert_eq!(body(&result)["returned"], json!(10));
        assert_eq!(body(&result)["truncated"], json!(true));
    }

    #[test]
    fn a_byte_budget_sheds_rows_the_item_count_would_have_kept() {
        let limits = OutputLimits {
            max_items: 1000,
            max_bytes: 200,
        };
        let result = page(&items(500), limits);
        let returned = body(&result)["returned"].as_u64().unwrap_or(u64::MAX);
        assert!(returned < 500, "expected shedding, kept {returned}");
        assert_eq!(body(&result)["truncated"], json!(true));
        assert!(body(&result).to_string().len() <= 200);
    }

    #[test]
    fn both_halves_of_a_structured_result_are_filled() {
        // Not every client reads structured_content, and the ones that do not see only the
        // text copy, so a result carrying just one of the two is invisible to half of them.
        let result = json(&json!({"domain": "example.com"}));
        assert!(result.structured_content.is_some());
        assert!(!result.content.is_empty());
    }
}
