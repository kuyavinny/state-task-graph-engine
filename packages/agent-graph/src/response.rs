use crate::error::AppError;
use crate::model::ErrorCode;
use serde::Serialize;

/// Standard JSON response envelope for all CLI output.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ResponseEnvelope<T: Serialize> {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    pub details: serde_json::Value,
}

impl<T: Serialize> ResponseEnvelope<T> {
    /// Create a success envelope.
    pub fn ok(revision: u64, data: T) -> Self {
        Self {
            ok: true,
            graph_revision: Some(revision),
            warnings: Some(Vec::new()),
            data: Some(data),
            error: None,
        }
    }

    /// Create a success envelope with warnings.
    #[allow(dead_code)]
    pub fn ok_with_warnings(revision: u64, data: T, warnings: Vec<String>) -> Self {
        Self {
            ok: true,
            graph_revision: Some(revision),
            warnings: if warnings.is_empty() {
                Some(Vec::new())
            } else {
                Some(warnings)
            },
            data: Some(data),
            error: None,
        }
    }

    /// Create a failure envelope from an AppError.
    ///
    /// `graph_revision` is included when the current revision is known
    /// (e.g., after graph load), enabling clients to retry with the correct revision.
    pub fn from_error(
        err: &AppError,
        graph_revision: Option<u64>,
    ) -> ResponseEnvelope<serde_json::Value> {
        ResponseEnvelope {
            ok: false,
            graph_revision,
            warnings: None,
            data: None,
            error: Some(ErrorBody {
                code: err.error_code(),
                message: err.to_string(),
                details: err.details(),
            }),
        }
    }
}

/// Shorthand for a value-less success envelope (init, etc).
impl ResponseEnvelope<serde_json::Value> {
    #[allow(dead_code)]
    pub fn ok_empty(revision: u64) -> Self {
        Self::ok(revision, serde_json::Value::Object(serde_json::Map::new()))
    }

    /// Pre-computed fallback JSON using the INTERNAL error code.
    /// Used when serde fails to serialize a normal error envelope.
    pub fn internal_fallback_json() -> String {
        let fallback: ResponseEnvelope<serde_json::Value> = ResponseEnvelope {
            ok: false,
            graph_revision: None,
            warnings: None,
            data: None,
            error: Some(ErrorBody {
                code: ErrorCode::Internal,
                message: "Failed to serialize error".to_string(),
                details: serde_json::Value::Object(serde_json::Map::new()),
            }),
        };
        serde_json::to_string(&fallback)
            .unwrap_or_else(|_| {
                r#"{"ok":false,"error":{"code":"INTERNAL","message":"Failed to serialize error","details":{}}}"#
                    .to_string()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    #[test]
    fn success_envelope_formats_correctly() {
        let env = ResponseEnvelope::ok(43, serde_json::json!({"initialized": true}));
        let json = serde_json::to_value(&env).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["graph_revision"], 43);
        assert_eq!(json["data"]["initialized"], true);
        assert!(json["warnings"].is_array());
    }

    #[test]
    fn failure_envelope_formats_correctly() {
        let err = AppError::StaleRevision {
            expected: 5,
            provided: 3,
        };
        let env = ResponseEnvelope::<serde_json::Value>::from_error(&err, None);
        let json = serde_json::to_value(&env).unwrap();
        assert_eq!(json["ok"], false);
        assert_eq!(json["error"]["code"], "STALE_REVISION");
        assert!(json["error"]["message"].is_string());
        assert_eq!(json["error"]["details"]["expected"], 5);
    }

    #[test]
    fn warnings_included_in_success() {
        let env = ResponseEnvelope::ok_with_warnings(
            10,
            serde_json::json!({"x": 1}),
            vec!["desync warning".to_string()],
        );
        let json = serde_json::to_value(&env).unwrap();
        assert_eq!(json["warnings"][0], "desync warning");
    }

    #[test]
    fn failure_envelope_no_data_field_when_null() {
        // Null fields should still serialize since we want a consistent shape
        let err = AppError::CycleDetected;
        let env = ResponseEnvelope::<serde_json::Value>::from_error(&err, None);
        let json = serde_json::to_string(&env).unwrap();
        // Verify the structure is present
        assert!(json.contains("\"ok\":false"));
        assert!(json.contains("\"error\""));
    }

    #[test]
    fn error_body_contains_all_fields() {
        let err = AppError::InvalidTransition {
            action: "complete".to_string(),
            current_status: "PENDING".to_string(),
        };
        let env = ResponseEnvelope::<serde_json::Value>::from_error(&err, None);
        let error_body = env.error.unwrap();
        assert_eq!(error_body.code, ErrorCode::InvalidTransition);
        assert!(error_body.message.contains("complete"));
        assert!(error_body.message.contains("PENDING"));
        assert_eq!(error_body.details["action"], "complete");
    }
}
