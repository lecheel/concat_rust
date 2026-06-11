//--+ src/stable_hash.rs

use sha2::{Digest, Sha256};

/// Compute a deterministic hash for a code body.
///
/// The hash includes both the filepath and the body content, so:
/// - Same body in different files → different hashes (scoped)
/// - Same body in same file → same hash always (stable)
/// - Different body in same file → different hash (correct)
///
/// Returns 12 hex characters (48 bits of entropy).
/// Collision probability: ~1 in 16 million at 16M items.
/// For code bodies this is negligible — typical projects have <100K bodies.
pub fn stable_hash_body(body: &str, filepath: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(filepath.as_bytes());
    hasher.update(&[0xFFu8]);
    hasher.update(body.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)[..12].to_string()
}

/// Compute a deterministic hash for an entire file's raw bytes.
///
/// Used by FileFingerprint for dirty detection.
/// Returns 24 hex characters (96 bits) for stronger uniqueness
/// since this drives sync decisions.
pub fn stable_hash_file(content: &[u8]) -> String {
    let result = Sha256::digest(content);
    format!("{:x}", result)[..24].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_input_same_hash() {
        let h1 = stable_hash_body("fn foo() {}", "src/main.rs");
        let h2 = stable_hash_body("fn foo() {}", "src/main.rs");
        assert_eq!(h1, h2, "Same input must produce same hash");
    }

    #[test]
    fn different_filepath_different_hash() {
        let h1 = stable_hash_body("fn foo() {}", "src/main.rs");
        let h2 = stable_hash_body("fn foo() {}", "src/utils.rs");
        assert_ne!(
            h1, h2,
            "Same body, different file must produce different hash"
        );
    }

    #[test]
    fn different_body_different_hash() {
        let h1 = stable_hash_body("fn foo() {}", "src/main.rs");
        let h2 = stable_hash_body("fn bar() {}", "src/main.rs");
        assert_ne!(h1, h2, "Different body must produce different hash");
    }

    #[test]
    fn hash_length_is_12() {
        let h = stable_hash_body("test", "test.rs");
        assert_eq!(h.len(), 12, "Body hash must be 12 hex chars");
    }

    #[test]
    fn file_hash_length_is_24() {
        let h = stable_hash_file(b"test content");
        assert_eq!(h.len(), 24, "File hash must be 24 hex chars");
    }

    #[test]
    fn file_hash_deterministic() {
        let h1 = stable_hash_file(b"test content");
        let h2 = stable_hash_file(b"test content");
        assert_eq!(h1, h2, "File hash must be deterministic");
    }

    #[test]
    fn separator_prevents_collision() {
        // filepath="ab" + body="c"  vs  filepath="a" + body="bc"
        // Without the separator, these would produce the same hash
        let h1 = stable_hash_body("c", "ab");
        let h2 = stable_hash_body("bc", "a");
        assert_ne!(h1, h2, "Separator must prevent boundary collision");
    }

    #[test]
    fn hash_stable_across_multiple_calls() {
        let body = r#"
            fn complex_function(x: i32, y: i32) -> i32 {
                let z = x + y;
                if z > 100 {
                    z * 2
                } else {
                    z + 1
                }
            }
        "#;
        let filepath = "core/src/math.rs";

        let mut hashes = Vec::new();
        for _ in 0..100 {
            hashes.push(stable_hash_body(body, filepath));
        }

        let first = &hashes[0];
        assert!(
            hashes.iter().all(|h| h == first),
            "Hash must be stable across calls"
        );
    }
}
