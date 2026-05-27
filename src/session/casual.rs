use crate::llm_client::{ContentBlock, Message};

/// Heuristic: returns true when the message looks like a fresh topic rather than
/// a continuation of the current conversation.
pub(super) fn is_topic_shift(input: &str, messages: &[Message]) -> bool {
    let user_texts: Vec<&str> = messages.iter()
        .filter(|m| m.role == "user")
        .flat_map(|m| m.content.iter())
        .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
        .collect();
    if user_texts.len() < 3 || input.len() < 40 { return false; }

    let lower = input.to_lowercase();
    let head: Vec<&str> = lower.split_whitespace().take(6).collect();
    let cont_words = ["it", "this", "that", "these", "those", "its", "above",
                      "also", "additionally", "furthermore", "now", "next"];
    if head.iter().any(|w| cont_words.contains(w)) { return false; }
    if lower.starts_with("and ") || lower.starts_with("but ") { return false; }

    let stop: std::collections::HashSet<&str> = [
        "the","a","an","and","or","but","in","on","at","to","for","of","with",
        "by","from","is","are","was","were","be","been","have","has","had","do",
        "does","did","will","would","could","should","may","might","can","this",
        "that","these","those","i","you","we","it","they","my","your","our",
        "please","help","make","add","create","want","need","like","just","how",
    ].iter().cloned().collect();

    let sig_words = |text: &str| -> std::collections::HashSet<String> {
        text.split_whitespace()
            .map(|w| w.to_lowercase().trim_matches(|c: char| !c.is_alphabetic()).to_string())
            .filter(|w| w.len() > 4 && !stop.contains(w.as_str()))
            .collect()
    };

    let recent: std::collections::HashSet<String> = user_texts.iter()
        .rev().take(3)
        .flat_map(|t| sig_words(t))
        .collect();
    let incoming = sig_words(input);

    if incoming.is_empty() || recent.is_empty() { return false; }
    let overlap = incoming.intersection(&recent).count();
    (overlap as f64 / incoming.len() as f64) < 0.15
}

pub(super) fn is_casual_message(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    if t.len() > 80 { return false; }
    let technical = [
        "fix", "bug", "code", "file", "function", "error", "test",
        "create", "add", "change", "update", "delete", "remove",
        "build", "run", "compile", "refactor", "write", "read",
        "show me", "explain", "how do", "what is", "why is",
        "implement", "debug", "check", "review", "edit", "find",
        "search", "list", "open", "close", "move", "rename",
        "push", "pull", "commit", "merge", "deploy", "release",
        "install", "revert", "reset", "branch", "checkout", "clone",
        "diff", "log", "stash", "tag", "patch",
    ];
    if technical.iter().any(|kw| t.contains(kw)) { return false; }
    let greetings = ["hi", "hello", "hey", "howdy", "greetings", "sup", "yo",
                     "how are you", "what's up", "whats up", "good morning",
                     "good evening", "good afternoon", "good night",
                     "thanks", "thank you", "ty", "thx",
                     "ok", "okay", "sure", "great", "nice", "cool", "awesome",
                     "sounds good", "perfect", "got it", "makes sense",
                     "what can you do", "what do you do"];
    greetings.iter().any(|g| t == *g
        || t.starts_with(&format!("{} ", g))
        || t.starts_with(&format!("{},", g))
        || t.starts_with(&format!("{}!", g)))
}

/// Returns true when the user's message is confirming or continuing a pending
/// action — e.g. "yes", "go ahead", "do it". Even if the text looks casual,
/// these replies need conversation history to be meaningful.
pub(super) fn is_action_confirmation(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    let confirmations = [
        "yes", "yeah", "yep", "yup", "y",
        "no", "nope", "nah", "n",
        "do it", "go ahead", "go for it", "proceed",
        "let's go", "lets go", "let's do it", "lets do it",
        "continue", "keep going", "carry on",
    ];
    confirmations.iter().any(|c| t == *c
        || t.starts_with(&format!("{} ", c))
        || t.starts_with(&format!("{},", c))
        || t.starts_with(&format!("{}!", c)))
}

/// Returns true when the last assistant message asked the user a question.
pub(super) fn last_message_was_question(messages: &[Message]) -> bool {
    messages.iter().rev()
        .find(|m| m.role == "assistant")
        .map(|m| {
            m.content.iter().any(|b| {
                if let ContentBlock::Text { text } = b {
                    let trimmed = text.trim_end_matches(|c: char| c.is_whitespace() || c == '*');
                    trimmed.ends_with('?')
                } else {
                    false
                }
            })
        })
        .unwrap_or(false)
}

/// Returns true when the message needs prior conversation history to be
/// answered correctly (e.g. confirmations, answers to a pending question).
/// Pure greetings are never context-dependent even if the model asked a question.
pub(super) fn needs_prior_context(text: &str, messages: &[Message]) -> bool {
    if is_action_confirmation(text) {
        return true;
    }
    if last_message_was_question(messages) && !is_pure_greeting(text) {
        return true;
    }
    false
}

/// True when the text is a bare greeting or social phrase that can never be
/// an answer to a question: "hi", "hello", "hey", "thanks", "cool", etc.
pub(super) fn is_pure_greeting(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    let greetings = ["hi", "hello", "hey", "howdy", "yo", "sup",
                     "thanks", "thank you", "ty", "thx",
                     "good morning", "good evening", "good afternoon", "good night",
                     "how are you", "what's up", "whats up",
                     "what can you do", "what do you do"];
    greetings.iter().any(|g| t == *g
        || t.starts_with(&format!("{} ", g))
        || t.starts_with(&format!("{},", g))
        || t.starts_with(&format!("{}!", g)))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod casual_tests {
    use super::is_casual_message;

    #[test]
    fn bare_greetings() {
        for msg in &["hi", "Hello", "HEY", "howdy", "yo", "sup"] {
            assert!(is_casual_message(msg), "{msg:?} should be casual");
        }
    }

    #[test]
    fn greeting_with_trailing_text() {
        assert!(is_casual_message("hi there"));
        assert!(is_casual_message("hello, world"));
        assert!(is_casual_message("hey!"));
        assert!(is_casual_message("good morning everyone"));
    }

    #[test]
    fn acknowledgements() {
        for msg in &["ok", "okay", "sure", "great", "thanks", "thank you", "ty",
                     "thx", "cool", "awesome", "sounds good", "perfect",
                     "got it", "makes sense"] {
            assert!(is_casual_message(msg), "{msg:?} should be casual");
        }
    }

    #[test]
    fn capability_question() {
        assert!(is_casual_message("what can you do"));
        assert!(is_casual_message("what do you do"));
    }

    #[test]
    fn mixed_case_and_whitespace() {
        assert!(is_casual_message("  Hi  "));
        assert!(is_casual_message("THANKS"));
        assert!(is_casual_message("Hey there!"));
    }

    #[test]
    fn technical_keywords_block_casual() {
        let cases = [
            "hi, can you fix this bug",
            "hey, show me the code",
            "hello, how do I build this",
            "ok, what is the error",
            "sure, create a test",
            "great, can you add a function",
        ];
        for msg in &cases {
            assert!(!is_casual_message(msg), "{msg:?} should NOT be casual");
        }
    }

    #[test]
    fn long_message_never_casual() {
        let long = "hi ".repeat(30);
        assert!(!is_casual_message(&long));
    }

    #[test]
    fn technical_standalone() {
        for msg in &["fix the login bug", "refactor this module",
                     "write a test", "explain this function", "find the error"] {
            assert!(!is_casual_message(msg), "{msg:?} should NOT be casual");
        }
    }

    #[test]
    fn not_a_known_greeting_prefix() {
        assert!(!is_casual_message("random stuff"));
        assert!(!is_casual_message("welcome back"));
        assert!(!is_casual_message("morning"));
    }

    #[test]
    fn git_ops_block_casual() {
        let cases = ["ok push it", "sure, pull", "great, commit now",
                     "ok deploy", "nice, merge it", "cool, revert that", "sure reset"];
        for msg in &cases {
            assert!(!is_casual_message(msg), "{msg:?} should NOT be casual");
        }
    }
}

#[cfg(test)]
mod prior_context_tests {
    use super::{is_pure_greeting, needs_prior_context};
    use crate::llm_client::{ContentBlock, Message};

    fn assistant_msg(text: &str) -> Message {
        Message { role: "assistant".to_string(), content: vec![ContentBlock::Text { text: text.to_string() }] }
    }

    #[test]
    fn hi_after_question_stays_casual() {
        let history = vec![
            Message::user_text("hi"),
            assistant_msg("Hello! How can I help you today?"),
        ];
        assert!(!needs_prior_context("hi", &history));
        assert!(!needs_prior_context("hello", &history));
        assert!(!needs_prior_context("hey", &history));
        assert!(!needs_prior_context("thanks", &history));
    }

    #[test]
    fn hi_with_no_history_not_context_dependent() {
        assert!(!needs_prior_context("hi", &[]));
    }

    #[test]
    fn yes_after_question_needs_context() {
        let history = vec![
            Message::user_text("refactor auth"),
            assistant_msg("Should I also update the tests?"),
        ];
        assert!(needs_prior_context("yes", &history));
        assert!(needs_prior_context("go ahead", &history));
        assert!(needs_prior_context("proceed", &history));
    }

    #[test]
    fn yes_without_question_still_needs_context() {
        assert!(needs_prior_context("yes", &[]));
        assert!(needs_prior_context("no", &[]));
    }

    #[test]
    fn short_answer_after_question_needs_context() {
        let history = vec![
            Message::user_text("fix the bug"),
            assistant_msg("Which file should I start with?"),
        ];
        assert!(needs_prior_context("main.rs", &history));
        assert!(needs_prior_context("the second one", &history));
    }

    #[test]
    fn pure_greetings_recognised() {
        for g in &["hi", "hello", "hey", "thanks", "thank you", "good morning"] {
            assert!(is_pure_greeting(g), "{g:?} should be a pure greeting");
        }
    }

    #[test]
    fn technical_text_not_pure_greeting() {
        assert!(!is_pure_greeting("main.rs"));
        assert!(!is_pure_greeting("yes"));
        assert!(!is_pure_greeting("the auth module"));
    }
}

#[cfg(test)]
mod context_tests {
    use super::{is_action_confirmation, last_message_was_question};
    use crate::llm_client::{ContentBlock, Message};

    #[test]
    fn action_confirmations_detected() {
        for msg in &["yes", "no", "y", "n", "do it", "go ahead", "proceed",
                     "continue", "let's go", "go for it"] {
            assert!(is_action_confirmation(msg), "{msg:?} should be action confirmation");
        }
    }

    #[test]
    fn social_words_not_confirmations() {
        for msg in &["thanks", "hi", "hello", "great", "cool", "amazing"] {
            assert!(!is_action_confirmation(msg), "{msg:?} should NOT be action confirmation");
        }
    }

    #[test]
    fn detects_question_in_last_assistant_message() {
        let messages = vec![
            Message { role: "user".to_string(), content: vec![ContentBlock::Text { text: "help".to_string() }] },
            Message { role: "assistant".to_string(), content: vec![ContentBlock::Text { text: "Should I push to main?".to_string() }] },
        ];
        assert!(last_message_was_question(&messages));
    }

    #[test]
    fn no_false_positive_when_no_question() {
        let messages = vec![
            Message { role: "user".to_string(), content: vec![ContentBlock::Text { text: "help".to_string() }] },
            Message { role: "assistant".to_string(), content: vec![ContentBlock::Text { text: "Done, pushed to main.".to_string() }] },
        ];
        assert!(!last_message_was_question(&messages));
    }
}
