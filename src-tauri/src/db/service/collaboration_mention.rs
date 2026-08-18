//! Structured Room mentions. Free-text `@word` is never a delivery target.
//!
//! Session chips serialize as `codeg://session/<id>`. The human operator is
//! `codeg://human` or `codeg://user`. File / commit / skill URIs stay in the
//! body and do not create deliveries.

use std::collections::BTreeSet;

const SESSION_URI: &str = "codeg://session/";
const HUMAN_URIS: [&str; 2] = ["codeg://human", "codeg://user"];

pub fn session_ids_from_structured_uris(text: &str) -> BTreeSet<i32> {
    let mut ids = BTreeSet::new();
    let mut rest = text;
    while let Some(idx) = rest.find(SESSION_URI) {
        let after = &rest[idx + SESSION_URI.len()..];
        let digits: String = after.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if let Ok(id) = digits.parse::<i32>() {
            if id > 0 {
                ids.insert(id);
            }
        }
        rest = if digits.is_empty() {
            &after[after.len().min(1)..]
        } else {
            &after[digits.len()..]
        };
    }
    ids
}

pub fn mentions_human_from_structured_uris(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    HUMAN_URIS.iter().any(|uri| lower.contains(uri))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prose_at_mentions_are_not_session_targets() {
        let ids = session_ids_from_structured_uris(
            "hey @alice and @all, see @/src/lib.rs and rust #[derive]",
        );
        assert!(ids.is_empty());
        assert!(!mentions_human_from_structured_uris(
            "ping @user in passing, not a chip"
        ));
    }

    #[test]
    fn structured_session_and_human_uris_are_mentions() {
        let ids = session_ids_from_structured_uris(
            "please look codeg://session/12 and also codeg://session/7.",
        );
        assert_eq!(ids.into_iter().collect::<Vec<_>>(), vec![7, 12]);
        assert!(mentions_human_from_structured_uris(
            "need a decision codeg://human today"
        ));
        assert!(mentions_human_from_structured_uris("codeg://user"));
    }

    #[test]
    fn file_uris_are_not_session_mentions() {
        assert!(session_ids_from_structured_uris("see file://src/foo.rs and codeg://file/bar").is_empty());
    }
}
