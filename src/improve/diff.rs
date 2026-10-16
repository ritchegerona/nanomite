//! Minimal line-based unified diff with zero dependencies.
//!
//! Only what the self-improvement review UI needs: common prefix/suffix
//! trimming plus unique-line anchors (patience-style) to split multi-region
//! edits into separate hunks, `@@` headers with 1-based line numbers and two
//! context lines per change region.
//!
//! Differences in the trailing newline only are ignored (`str::lines`
//! semantics); the diff is informational and the proposal layer compares
//! bytes separately.

use std::collections::HashMap;

/// Context lines shown around each change region.
const CONTEXT: usize = 2;

/// Renders a unified diff between `old` and `new`, labelling the file `path`.
/// Returns an empty string when the inputs are line-identical.
pub fn unified_diff(old: &str, new: &str, path: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut ops = Vec::new();
    walk(&old_lines, &new_lines, &mut ops);
    if ops.iter().all(|op| matches!(op, Op::Equal(_))) {
        return String::new();
    }
    render(&ops, path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op<'a> {
    Equal(&'a str),
    Del(&'a str),
    Add(&'a str),
}

/// Walks both line slices, emitting an ordered op list. Common prefix and
/// suffix are trimmed first; the remaining middle is split on unique-line
/// anchors so two distant edits do not collapse into one hunk.
fn walk<'a>(old: &[&'a str], new: &[&'a str], ops: &mut Vec<Op<'a>>) {
    let prefix = old
        .iter()
        .zip(new.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = common_suffix(&old[prefix..], &new[prefix..]);
    for line in &old[..prefix] {
        ops.push(Op::Equal(line));
    }
    let o = &old[prefix..old.len() - suffix];
    let n = &new[prefix..new.len() - suffix];
    if !o.is_empty() || !n.is_empty() {
        let anchors = unique_anchors(o, n);
        if anchors.is_empty() {
            for line in o {
                ops.push(Op::Del(line));
            }
            for line in n {
                ops.push(Op::Add(line));
            }
        } else {
            let (mut oi, mut ni) = (0usize, 0usize);
            for (ai, aj) in anchors {
                walk(&o[oi..ai], &n[ni..aj], ops);
                ops.push(Op::Equal(o[ai]));
                oi = ai + 1;
                ni = aj + 1;
            }
            walk(&o[oi..], &n[ni..], ops);
        }
    }
    for line in &old[old.len() - suffix..] {
        ops.push(Op::Equal(line));
    }
}

fn common_suffix(a: &[&str], b: &[&str]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

/// Line pairs that appear exactly once on each side and are equal — anchors
/// for splitting the edit into independent regions. The result is the longest
/// chain whose positions increase on both sides.
fn unique_anchors(o: &[&str], n: &[&str]) -> Vec<(usize, usize)> {
    let mut o_count: HashMap<&str, usize> = HashMap::new();
    for &line in o {
        *o_count.entry(line).or_insert(0) += 1;
    }
    let mut n_count: HashMap<&str, usize> = HashMap::new();
    for &line in n {
        *n_count.entry(line).or_insert(0) += 1;
    }
    let mut n_pos: HashMap<&str, usize> = HashMap::new();
    for (i, &line) in n.iter().enumerate() {
        if n_count.get(&line).copied().unwrap_or(0) == 1 {
            n_pos.insert(line, i);
        }
    }
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (i, &line) in o.iter().enumerate() {
        if o_count.get(&line).copied().unwrap_or(0) == 1 {
            if let Some(&j) = n_pos.get(&line) {
                pairs.push((i, j));
            }
        }
    }
    longest_increasing_chain(&pairs)
}

/// Longest subsequence of `pairs` (sorted by `.0`) whose `.1` strictly increases.
fn longest_increasing_chain(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let len = pairs.len();
    if len == 0 {
        return Vec::new();
    }
    let mut tails: Vec<usize> = Vec::new();
    let mut prev = vec![usize::MAX; len];
    for (i, &(_, j)) in pairs.iter().enumerate() {
        let pos = tails.partition_point(|&idx| pairs[idx].1 < j);
        if pos == tails.len() {
            tails.push(i);
        } else {
            tails[pos] = i;
        }
        if pos > 0 {
            prev[i] = tails[pos - 1];
        }
    }
    let mut out = Vec::with_capacity(tails.len());
    let mut idx = tails[tails.len() - 1];
    while idx != usize::MAX {
        out.push(pairs[idx]);
        idx = prev[idx];
    }
    out.reverse();
    out
}

fn render(ops: &[Op], path: &str) -> String {
    let mut out = String::from("--- ");
    out.push_str(path);
    out.push_str("\n+++ ");
    out.push_str(path);
    out.push('\n');

    // 1-based line number each op would consume, recorded before it runs.
    let mut numbers = Vec::with_capacity(ops.len());
    let (mut old_no, mut new_no) = (1usize, 1usize);
    for op in ops {
        numbers.push((old_no, new_no));
        match op {
            Op::Equal(_) => {
                old_no += 1;
                new_no += 1;
            }
            Op::Del(_) => old_no += 1,
            Op::Add(_) => new_no += 1,
        }
    }

    // Group change ops; merge groups separated by at most 2*CONTEXT equal lines.
    let changes: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, op)| !matches!(op, Op::Equal(_)))
        .map(|(i, _)| i)
        .collect();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for &ci in &changes {
        match groups.last_mut() {
            Some((_, last)) if ci - *last <= 2 * CONTEXT + 1 => *last = ci,
            _ => groups.push((ci, ci)),
        }
    }

    for (first_change, last_change) in groups {
        let start = first_change.saturating_sub(CONTEXT);
        let end = (last_change + CONTEXT).min(ops.len() - 1);
        let slice = &ops[start..=end];
        let old_count = slice.iter().filter(|op| !matches!(op, Op::Add(_))).count();
        let new_count = slice.iter().filter(|op| !matches!(op, Op::Del(_))).count();
        let first_old = slice
            .iter()
            .zip(&numbers[start..=end])
            .find_map(|(op, nums)| (!matches!(op, Op::Add(_))).then_some(*nums));
        let first_new = slice
            .iter()
            .zip(&numbers[start..=end])
            .find_map(|(op, nums)| (!matches!(op, Op::Del(_))).then_some(*nums));
        // A zero-length range anchors at the line *before* it (git convention).
        let old_start = match first_old {
            Some((o, _)) => o,
            None => numbers[start].0.saturating_sub(1),
        };
        let new_start = match first_new {
            Some((_, n)) => n,
            None => numbers[start].1.saturating_sub(1),
        };
        out.push_str("@@ -");
        push_range(&mut out, old_start, old_count);
        out.push_str(" +");
        push_range(&mut out, new_start, new_count);
        out.push_str(" @@\n");
        for op in slice {
            match op {
                Op::Equal(line) => {
                    out.push(' ');
                    out.push_str(line);
                }
                Op::Del(line) => {
                    out.push('-');
                    out.push_str(line);
                }
                Op::Add(line) => {
                    out.push('+');
                    out.push_str(line);
                }
            }
            out.push('\n');
        }
    }
    out
}

/// Git header style: `@@ -5 +7,2 @@` — the `,1` is omitted for single lines.
fn push_range(out: &mut String, start: usize, count: usize) {
    out.push_str(&start.to_string());
    if count != 1 {
        out.push(',');
        out.push_str(&count.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::unified_diff;

    fn lines(count: usize) -> String {
        (0..count)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    #[test]
    fn identical_input_yields_empty_diff() {
        assert_eq!(unified_diff("a\nb\n", "a\nb\n", "f"), "");
        assert_eq!(unified_diff("", "", "f"), "");
    }

    #[test]
    fn single_edit_produces_one_hunk_with_correct_counts() {
        let expected = "--- f\n+++ f\n@@ -1,5 +1,5 @@\n a\n b\n-c\n+X\n d\n e\n";
        assert_eq!(
            unified_diff("a\nb\nc\nd\ne\n", "a\nb\nX\nd\ne\n", "f"),
            expected
        );
    }

    #[test]
    fn separated_edits_produce_two_hunks() {
        let old = lines(20);
        let mut new_lines: Vec<String> = (0..20).map(|i| format!("l{i}")).collect();
        new_lines[2] = "x2".to_string();
        new_lines[15] = "x15".to_string();
        let new = new_lines.join("\n") + "\n";
        let expected = concat!(
            "--- f\n",
            "+++ f\n",
            "@@ -1,5 +1,5 @@\n",
            " l0\n",
            " l1\n",
            "-l2\n",
            "+x2\n",
            " l3\n",
            " l4\n",
            "@@ -14,5 +14,5 @@\n",
            " l13\n",
            " l14\n",
            "-l15\n",
            "+x15\n",
            " l16\n",
            " l17\n",
        );
        assert_eq!(unified_diff(&old, &new, "f"), expected);
    }

    #[test]
    fn insertion_and_deletion_headers_are_git_shaped() {
        let inserted = "--- f\n+++ f\n@@ -1,2 +1,3 @@\n a\n+z\n b\n";
        assert_eq!(unified_diff("a\nb\n", "a\nz\nb\n", "f"), inserted);
        let deleted = "--- f\n+++ f\n@@ -1,3 +1,2 @@\n a\n-z\n b\n";
        assert_eq!(unified_diff("a\nz\nb\n", "a\nb\n", "f"), deleted);
    }

    #[test]
    fn unicode_lines_survive_the_round_trip() {
        let expected = "--- ünïcode.md\n+++ ünïcode.md\n@@ -1,2 +1,2 @@\n héllo\n-wörld\n+wörld✓\n";
        assert_eq!(
            unified_diff("héllo\nwörld\n", "héllo\nwörld✓\n", "ünïcode.md"),
            expected
        );
    }
}
