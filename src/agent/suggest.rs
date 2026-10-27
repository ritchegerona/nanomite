//! opencode-style follow-up suggestions.
//!
//! When a turn finishes, the app fires a tiny side-call (no tools, clipped
//! context) asking the model for a few short prompts the user would most
//! plausibly send next. The UI mounts them as a slim strip above the
//! composer: `Tab` cycles, `Enter` sends the highlighted one, `Esc`
//! dismisses, and typing anything dismisses and types normally.
//!
//! Only the pure logic lives here (prompt shape + reply parsing); the app
//! owns the channel, the generation counter and the rendering.

/// Result of one follow-up suggestion side-call, sent back to the UI thread.
#[derive(Debug, Clone)]
pub struct SuggestResult {
    /// Generation of the request that produced this result. A straggler from
    /// an older turn (its gen no longer current) is dropped by the app.
    pub gen: u64,
    /// Cleaned-up suggestions, ready to render and submit verbatim.
    pub items: Vec<String>,
}

/// System prompt for the side-call. Deliberately strict about shape — one
/// suggestion per line and nothing else — so parsing stays trivial and any
/// instruction-following model works.
pub const SYSTEM_PROMPT: &str = "\
You generate follow-up prompt suggestions for a terminal coding agent. \
Given the last exchange, reply with exactly 3 short prompts the user would \
most plausibly send next. Rules:
- one per line, no numbering, no bullets, no quotes
- at most 8 words each, imperative voice
- concrete next actions (verify, test, commit, refactor, explore), not questions
- output ONLY the 3 lines";

/// How much of each side of the finished exchange the side-call sees. The
/// gist is enough for good suggestions; whole transcripts would only burn
/// tokens on an extra request every turn.
const CONTEXT_CHARS: usize = 2_000;

/// Upper bound for one suggestion; longer lines mean the model ignored the
/// shape instructions, so they are hard-clipped.
const MAX_ITEM_CHARS: usize = 80;

/// Builds the user-side prompt from the finished exchange.
pub fn build_request(user_text: &str, assistant_text: &str) -> String {
    format!(
        "The user asked:\n{}\n\nThe agent answered:\n{}\n\nReply with the 3 suggestion lines now.",
        clip(user_text, CONTEXT_CHARS),
        clip(assistant_text, CONTEXT_CHARS)
    )
}

/// Parses a raw completion into at most `max` suggestions: strips bullets,
/// numbering, wrapping quotes and code fences, drops blanks and duplicates,
/// clamps each item to [`MAX_ITEM_CHARS`].
pub fn parse_reply(raw: &str, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let mut s = line.trim();
        if s.starts_with("```") {
            continue;
        }
        // One leading bullet / numbering prefix, then whitespace.
        s = s
            .trim_start_matches(['-', '*', '•', '·', '›', '>', '▸', '‣', '–'])
            .trim();
        if let Some(rest) = s
            .strip_prefix(|c: char| c.is_ascii_digit())
            .and_then(|r| r.trim_start().strip_prefix(['.', ')']))
        {
            s = rest.trim_start();
        }
        // Wrapping quotes or backticks around the whole line.
        for pair in [('"', '"'), ('`', '`'), ('‘', '’')] {
            if s.len() >= 2 && s.starts_with(pair.0) && s.ends_with(pair.1) {
                s = s[1..s.len() - pair.1.len_utf8()].trim();
            }
        }
        if s.is_empty() {
            continue;
        }
        // Case-insensitive dedupe: the same action twice helps nobody. Long
        // lines mean the model ignored the shape rules, so they are clipped.
        let item = if s.chars().count() > MAX_ITEM_CHARS {
            format!(
                "{}…",
                s.chars().take(MAX_ITEM_CHARS - 1).collect::<String>()
            )
        } else {
            s.to_string()
        };
        if out.iter().any(|i| i.eq_ignore_ascii_case(&item)) {
            continue;
        }
        out.push(item);
        if out.len() == max {
            break;
        }
    }
    out
}

/// Hard character clip that keeps the text a valid prompt fragment.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strips_bullets_numbering_and_quotes() {
        let raw =
            "1. run the tests\n- commit the fix\n* `add a changelog entry`\n\"deploy to staging\"";
        assert_eq!(
            parse_reply(raw, 4),
            vec![
                "run the tests",
                "commit the fix",
                "add a changelog entry",
                "deploy to staging"
            ]
        );
    }

    #[test]
    fn parse_drops_blanks_fences_and_duplicates() {
        let raw = "```\nrun the tests\n\nRun the tests\n```\ncommit the fix";
        assert_eq!(parse_reply(raw, 3), vec!["run the tests", "commit the fix"]);
    }

    #[test]
    fn parse_caps_at_max_and_clips_long_lines() {
        let raw = "a\nb\nc\nd";
        assert_eq!(parse_reply(raw, 3).len(), 3);
        let long = "x".repeat(200);
        let parsed = parse_reply(&long, 1);
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].chars().count() <= MAX_ITEM_CHARS);
        assert!(parsed[0].ends_with('…'));
    }

    #[test]
    fn parse_empty_reply_yields_no_items() {
        assert!(parse_reply("", 3).is_empty());
        assert!(parse_reply("   \n  \n", 3).is_empty());
    }

    #[test]
    fn request_carries_both_sides_and_clips() {
        let req = build_request("fix the bug", "done, here is the diff");
        assert!(req.contains("fix the bug"));
        assert!(req.contains("done, here is the diff"));
        let long_user = "u".repeat(5_000);
        let req = build_request(&long_user, "ok");
        assert!(req.chars().count() < CONTEXT_CHARS * 2 + 200);
        assert!(req.contains('…'));
    }
}
