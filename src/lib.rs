//! Report-once HTTP error handling.
//!
//! The contract is: a handler that fails emits **one** stderr line of JSON
//! carrying the technical detail under a fresh 8-hex `ref`, and writes a clean
//! `{"error","ref"}` body to the client. The `ref` is the join key between a
//! user's report and the exact log line.
//!
//! The wire format is language-portable — the same JSON line
//! `{"level":"error","msg":"request failed","app","ref","method","path",
//!  "status","user","err"}` across every service. `level` is lowercased
//! ("error" not "ERROR") so a single `| json | level="error"` log query works
//! everywhere.
//!
//! Framework-agnostic on purpose. axum / actix / hyper apps wrap these
//! primitives in their own response type.

use std::hash::{BuildHasher, Hasher};

/// Returns a short, user-quotable reference id (8 hex chars). Correlation
/// token only — not a security or uniqueness guarantee. Uses the stdlib
/// random-seeded hasher so the crate stays zero-dep on randomness.
pub fn new_ref() -> String {
    let n = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    format!("{:08x}", n as u32)
}

/// Emits the standard error log line to stderr and returns the generated
/// `ref`. `user` should be the caller identity (email) or `"-"` if unknown.
/// `err` is the raw internal detail — never include it in the client body.
///
/// `path` should be the low-cardinality route template (`/api/v1/sites/{slug}`,
/// not the raw URL) to keep Loki cardinality bounded.
pub fn emit_failed(app: &str, method: &str, path: &str, status: u16, user: &str, err: &str) -> String {
    let r = new_ref();
    let line = serde_json::json!({
        "level": level_for(status),
        "msg": "request failed",
        "app": app,
        "ref": r,
        "method": method,
        "path": path,
        "status": status,
        "user": if user.is_empty() { "-" } else { user },
        "err": err,
    });
    eprintln!("{line}");
    r
}

/// Returns the JSON value to write as the client response body for a failure
/// (`{"error","ref"}`). `user_msg` is the clean, localized message; `ref_` is
/// the value returned by [`emit_failed`] (or [`new_ref`] if you log
/// separately).
pub fn fail_body(user_msg: &str, ref_: &str) -> serde_json::Value {
    serde_json::json!({ "error": user_msg, "ref": ref_ })
}

/// `level` field policy: server errors get `"error"`, client errors get
/// `"warn"`, everything else `"info"`. Matches the Go convention's effective
/// behavior (Go uses slog.Error; we split client vs server to dedupe pager
/// noise).
fn level_for(status: u16) -> &'static str {
    if status >= 500 {
        "error"
    } else if status >= 400 {
        "warn"
    } else {
        "info"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ref_is_8_hex_chars() {
        for _ in 0..100 {
            let r = new_ref();
            assert_eq!(r.len(), 8, "ref should be 8 chars, got {r:?}");
            assert!(r.chars().all(|c| c.is_ascii_hexdigit()), "non-hex: {r:?}");
        }
    }

    #[test]
    fn fail_body_shape() {
        let v = fail_body("not allowed", "deadbeef");
        assert_eq!(v["error"], "not allowed");
        assert_eq!(v["ref"], "deadbeef");
        assert_eq!(v.as_object().unwrap().len(), 2);
    }

    #[test]
    fn level_for_buckets() {
        assert_eq!(level_for(200), "info");
        assert_eq!(level_for(404), "warn");
        assert_eq!(level_for(500), "error");
        assert_eq!(level_for(503), "error");
    }
}
