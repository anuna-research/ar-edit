use crate::models::Transcript;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Find a contiguous word range in the transcript that matches the given text.
///
/// Returns `Some((from_word_index, to_word_index))` on success, or `None`
/// when the text cannot be matched against the transcript.
///
/// The algorithm first tries an exact normalised match (lowercase, stripped
/// punctuation).  If that fails it falls back to fuzzy matching that tolerates
/// minor per-word typos (edit distance ≤ 1 for words of 4+ characters).
/// At least 80 % of input words must match for the fuzzy path to succeed.
pub fn match_text(text: &str, transcript: &Transcript) -> Option<(u32, u32)> {
    let input_words = tokenize(text);
    if input_words.is_empty() {
        return None;
    }

    // Collect all words from transcript with their global indices.
    let all_words: Vec<(u32, String)> = transcript
        .segments
        .iter()
        .flat_map(|s| s.words.iter())
        .map(|w| (w.index, normalize_word(&w.text)))
        .collect();

    if all_words.is_empty() || input_words.len() > all_words.len() {
        return None;
    }

    let window = input_words.len();

    // Phase 1: exact normalised sliding-window match.
    for i in 0..=(all_words.len() - window) {
        let mut ok = true;
        for j in 0..window {
            if all_words[i + j].1 != input_words[j] {
                ok = false;
                break;
            }
        }
        if ok {
            return Some((all_words[i].0, all_words[i + window - 1].0));
        }
    }

    // Phase 2: fuzzy sliding-window match.
    let threshold = (window as f64 * 0.8).ceil() as usize;
    let mut best_score: usize = 0;
    let mut best_pos: Option<usize> = None;

    for i in 0..=(all_words.len() - window) {
        let mut score: usize = 0;
        for j in 0..window {
            if words_match_fuzzy(&all_words[i + j].1, &input_words[j]) {
                score += 1;
            }
        }
        if score > best_score {
            best_score = score;
            best_pos = Some(i);
        }
    }

    if let Some(pos) = best_pos {
        if best_score >= threshold {
            return Some((all_words[pos].0, all_words[pos + window - 1].0));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Split `text` on whitespace and normalise each token.
fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(normalize_word)
        .filter(|w| !w.is_empty())
        .collect()
}

/// Lower-case the word and strip leading/trailing punctuation characters.
fn normalize_word(word: &str) -> String {
    let lower = word.to_lowercase();
    lower
        .trim_matches(|c: char| c.is_ascii_punctuation())
        .to_string()
}

/// Two normalised words "match" if they are identical, or if both are at
/// least 4 characters long and their Levenshtein distance is ≤ 1.
fn words_match_fuzzy(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.len() >= 4 && b.len() >= 4 {
        return levenshtein(a, b) <= 1;
    }
    false
}

/// Classic two-row Levenshtein distance.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    let mut prev = vec![0usize; n + 1];
    let mut curr = vec![0usize; n + 1];

    for (j, val) in prev.iter_mut().enumerate().take(n + 1) {
        *val = j;
    }

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Transcript, TranscriptSegment, Word};

    fn make_transcript(words: &[&str]) -> Transcript {
        let words_vec: Vec<Word> = words
            .iter()
            .enumerate()
            .map(|(i, text)| Word {
                index: i as u32,
                text: text.to_string(),
                start_ms: (i as u64) * 500,
                end_ms: (i as u64) * 500 + 400,
                confidence: 0.95,
            })
            .collect();

        let text = words.join(" ");
        Transcript {
            source_id: "src-001".into(),
            model: "base".into(),
            language: "en".into(),
            duration_ms: words_vec.last().map_or(0, |w| w.end_ms),
            word_count: words_vec.len() as u32,
            segments: vec![TranscriptSegment {
                index: 0,
                start_ms: 0,
                end_ms: words_vec.last().map_or(0, |w| w.end_ms),
                text,
                words: words_vec,
            }],
        }
    }

    fn make_transcript_multi_segment(segments: Vec<Vec<&str>>) -> Transcript {
        let mut all_segments = Vec::new();
        let mut global_idx: u32 = 0;
        let mut duration_ms: u64 = 0;

        for (seg_idx, words) in segments.iter().enumerate() {
            let words_vec: Vec<Word> = words
                .iter()
                .map(|text| {
                    let w = Word {
                        index: global_idx,
                        text: text.to_string(),
                        start_ms: (global_idx as u64) * 500,
                        end_ms: (global_idx as u64) * 500 + 400,
                        confidence: 0.95,
                    };
                    global_idx += 1;
                    if w.end_ms > duration_ms {
                        duration_ms = w.end_ms;
                    }
                    w
                })
                .collect();

            let text = words.join(" ");
            let start = words_vec.first().map_or(0, |w| w.start_ms);
            let end = words_vec.last().map_or(0, |w| w.end_ms);
            all_segments.push(TranscriptSegment {
                index: seg_idx as u32,
                start_ms: start,
                end_ms: end,
                text,
                words: words_vec,
            });
        }

        Transcript {
            source_id: "src-001".into(),
            model: "base".into(),
            language: "en".into(),
            duration_ms,
            word_count: global_idx,
            segments: all_segments,
        }
    }

    // -- normalize_word -------------------------------------------------------

    #[test]
    fn normalize_strips_punctuation() {
        assert_eq!(normalize_word("Hello,"), "hello");
        assert_eq!(normalize_word("\"world\""), "world");
        assert_eq!(normalize_word("(test)"), "test");
        assert_eq!(normalize_word("we're"), "we're");
    }

    #[test]
    fn normalize_lowercases() {
        assert_eq!(normalize_word("Welcome"), "welcome");
        assert_eq!(normalize_word("TODAY"), "today");
    }

    // -- levenshtein ----------------------------------------------------------

    #[test]
    fn levenshtein_identical() {
        assert_eq!(levenshtein("hello", "hello"), 0);
    }

    #[test]
    fn levenshtein_one_edit() {
        assert_eq!(levenshtein("hello", "helo"), 1);
        assert_eq!(levenshtein("interview", "intervew"), 1);
        assert_eq!(levenshtein("climate", "clmate"), 1);
    }

    #[test]
    fn levenshtein_two_edits() {
        assert_eq!(levenshtein("hello", "hllo"), 1); // one deletion
        assert_eq!(levenshtein("hello", "hlo"), 2); // two deletions
    }

    // -- match_text: exact matches --------------------------------------------

    #[test]
    fn exact_match_full_transcript() {
        let t = make_transcript(&["Welcome", "to", "the", "interview"]);
        let result = match_text("Welcome to the interview", &t);
        assert_eq!(result, Some((0, 3)));
    }

    #[test]
    fn exact_match_beginning() {
        let t = make_transcript(&["Welcome", "to", "the", "interview", "today", "we", "talk"]);
        let result = match_text("Welcome to the interview", &t);
        assert_eq!(result, Some((0, 3)));
    }

    #[test]
    fn exact_match_middle() {
        let t = make_transcript(&["Welcome", "to", "the", "interview", "today", "we", "talk"]);
        let result = match_text("the interview today", &t);
        assert_eq!(result, Some((2, 4)));
    }

    #[test]
    fn exact_match_end() {
        let t = make_transcript(&["Welcome", "to", "the", "interview", "today", "we", "talk"]);
        let result = match_text("today we talk", &t);
        assert_eq!(result, Some((4, 6)));
    }

    #[test]
    fn exact_match_case_insensitive() {
        let t = make_transcript(&["Welcome", "to", "the", "Interview"]);
        let result = match_text("welcome to the interview", &t);
        assert_eq!(result, Some((0, 3)));
    }

    #[test]
    fn exact_match_ignores_punctuation() {
        let t = make_transcript(&["Hello,", "world!"]);
        let result = match_text("Hello world", &t);
        assert_eq!(result, Some((0, 1)));
    }

    #[test]
    fn exact_match_across_segments() {
        let t = make_transcript_multi_segment(vec![
            vec!["Welcome", "to", "the", "interview"],
            vec!["today", "we", "talk"],
        ]);
        // Match spanning both segments
        let result = match_text("the interview today we", &t);
        assert_eq!(result, Some((2, 5)));
    }

    #[test]
    fn exact_match_single_word() {
        let t = make_transcript(&["Welcome", "to", "the", "interview"]);
        let result = match_text("interview", &t);
        assert_eq!(result, Some((3, 3)));
    }

    // -- match_text: fuzzy matches --------------------------------------------

    #[test]
    fn fuzzy_match_one_typo() {
        let t = make_transcript(&["Welcome", "to", "the", "interview", "today"]);
        // "intervew" is 1 edit from "interview"
        let result = match_text("Welcome to the intervew today", &t);
        assert_eq!(result, Some((0, 4)));
    }

    #[test]
    fn fuzzy_match_threshold_met() {
        // 5 words, threshold = ceil(5 * 0.8) = 4
        let t = make_transcript(&["Welcome", "to", "the", "interview", "today"]);
        // 4/5 match (one wrong short word "xx" won't fuzzy-match "to")
        let result = match_text("Welcome xx the interview today", &t);
        assert_eq!(result, Some((0, 4)));
    }

    #[test]
    fn fuzzy_match_threshold_not_met() {
        let t = make_transcript(&["Welcome", "to", "the", "interview", "today"]);
        // Only 1/5 match — well below threshold
        let result = match_text("Goodbye from our discussion yesterday", &t);
        assert_eq!(result, None);
    }

    // -- match_text: no match -------------------------------------------------

    #[test]
    fn no_match_completely_different() {
        let t = make_transcript(&["Welcome", "to", "the", "interview"]);
        let result = match_text("This is something else entirely", &t);
        assert_eq!(result, None);
    }

    #[test]
    fn no_match_empty_text() {
        let t = make_transcript(&["Welcome", "to"]);
        let result = match_text("", &t);
        assert_eq!(result, None);
    }

    #[test]
    fn no_match_whitespace_only() {
        let t = make_transcript(&["Welcome", "to"]);
        let result = match_text("   \n  \t  ", &t);
        assert_eq!(result, None);
    }

    #[test]
    fn no_match_text_longer_than_transcript() {
        let t = make_transcript(&["Hello"]);
        let result = match_text("Hello world foo bar baz", &t);
        assert_eq!(result, None);
    }

    #[test]
    fn no_match_empty_transcript() {
        let t = Transcript {
            source_id: "src-001".into(),
            model: "base".into(),
            language: "en".into(),
            duration_ms: 0,
            word_count: 0,
            segments: vec![],
        };
        let result = match_text("Hello world", &t);
        assert_eq!(result, None);
    }

    // -- split/merge scenarios ------------------------------------------------

    #[test]
    fn split_block_first_half() {
        let t = make_transcript(&[
            "Welcome",
            "to",
            "the",
            "interview",
            "today",
            "we're",
            "going",
            "to",
            "talk",
            "about",
        ]);
        // User kept the first half
        let result = match_text("Welcome to the interview", &t);
        assert_eq!(result, Some((0, 3)));
    }

    #[test]
    fn split_block_second_half() {
        let t = make_transcript(&[
            "Welcome",
            "to",
            "the",
            "interview",
            "today",
            "we're",
            "going",
            "to",
            "talk",
            "about",
        ]);
        // User split after "interview" — orphaned second half
        let result = match_text("today we're going to talk about", &t);
        assert_eq!(result, Some((4, 9)));
    }

    #[test]
    fn merged_blocks_across_segments() {
        let t = make_transcript_multi_segment(vec![
            vec!["Welcome", "to", "the", "interview"],
            vec!["today", "we", "talk", "about"],
            vec!["climate", "policy"],
        ]);
        // Merged first two segments
        let result = match_text("Welcome to the interview today we talk about", &t);
        assert_eq!(result, Some((0, 7)));
    }

    #[test]
    fn merged_all_segments() {
        let t = make_transcript_multi_segment(vec![
            vec!["Welcome", "to"],
            vec!["the", "interview"],
            vec!["today"],
        ]);
        let result = match_text("Welcome to the interview today", &t);
        assert_eq!(result, Some((0, 4)));
    }
}
