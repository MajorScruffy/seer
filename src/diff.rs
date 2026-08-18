use std::collections::HashMap;

/// Unified diff. Empty string iff `a == b`.
///
/// Byte-identical to Python `difflib.unified_diff(..., n=3, lineterm='\\n')`
/// when `a != b`.
pub fn diff_text(a: &str, b: &str, name_a: &str, name_b: &str) -> String {
    if a == b {
        return String::new();
    }
    unified_diff(&split_keepends(a), &split_keepends(b), name_a, name_b)
}

fn split_keepends(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, _) in s.match_indices('\n') {
        lines.push(&s[start..=i]);
        start = i + 1;
    }
    if start < s.len() {
        lines.push(&s[start..]);
    }
    lines
}

fn unified_diff(a: &[&str], b: &[&str], name_a: &str, name_b: &str) -> String {
    let mut out = String::new();
    let mut started = false;
    for group in grouped_opcodes(a, b, 3) {
        if !started {
            started = true;
            out.push_str("--- ");
            out.push_str(name_a);
            out.push('\n');
            out.push_str("+++ ");
            out.push_str(name_b);
            out.push('\n');
        }
        let first = group[0];
        let last = group[group.len() - 1];
        out.push_str("@@ -");
        out.push_str(&format_range_unified(first.i1, last.i2));
        out.push_str(" +");
        out.push_str(&format_range_unified(first.j1, last.j2));
        out.push_str(" @@\n");
        for op in group {
            match op.tag {
                Tag::Equal => {
                    for line in &a[op.i1..op.i2] {
                        out.push(' ');
                        out.push_str(line);
                    }
                }
                Tag::Replace | Tag::Delete => {
                    for line in &a[op.i1..op.i2] {
                        out.push('-');
                        out.push_str(line);
                    }
                    if op.tag == Tag::Replace {
                        for line in &b[op.j1..op.j2] {
                            out.push('+');
                            out.push_str(line);
                        }
                    }
                }
                Tag::Insert => {
                    for line in &b[op.j1..op.j2] {
                        out.push('+');
                        out.push_str(line);
                    }
                }
            }
        }
    }
    out
}

/// Python 3.12+ `difflib._format_range_unified`.
fn format_range_unified(start: usize, stop: usize) -> String {
    let mut beginning = start + 1;
    let length = stop - start;
    if length == 1 {
        return beginning.to_string();
    }
    if length == 0 {
        beginning -= 1;
    }
    format!("{beginning},{length}")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Equal,
    Replace,
    Delete,
    Insert,
}

#[derive(Clone, Copy)]
struct Opcode {
    tag: Tag,
    i1: usize,
    i2: usize,
    j1: usize,
    j2: usize,
}

/// `SequenceMatcher(None, a, b).get_grouped_opcodes(n)` (autojunk on).
fn grouped_opcodes(a: &[&str], b: &[&str], n: usize) -> Vec<Vec<Opcode>> {
    let mut codes = opcodes(a, b);
    if codes.is_empty() {
        codes.push(Opcode {
            tag: Tag::Equal,
            i1: 0,
            i2: 1,
            j1: 0,
            j2: 1,
        });
    }
    if codes[0].tag == Tag::Equal {
        let op = &mut codes[0];
        op.i1 = op.i1.max(op.i2.saturating_sub(n));
        op.j1 = op.j1.max(op.j2.saturating_sub(n));
    }
    if codes.last().is_some_and(|op| op.tag == Tag::Equal) {
        let last = codes.len() - 1;
        let op = &mut codes[last];
        op.i2 = op.i2.min(op.i1 + n);
        op.j2 = op.j2.min(op.j1 + n);
    }

    let nn = n + n;
    let mut groups = Vec::new();
    let mut group = Vec::new();
    for mut op in codes {
        if op.tag == Tag::Equal && op.i2 - op.i1 > nn {
            group.push(Opcode {
                tag: Tag::Equal,
                i1: op.i1,
                i2: op.i2.min(op.i1 + n),
                j1: op.j1,
                j2: op.j2.min(op.j1 + n),
            });
            groups.push(std::mem::take(&mut group));
            op.i1 = op.i1.max(op.i2.saturating_sub(n));
            op.j1 = op.j1.max(op.j2.saturating_sub(n));
        }
        group.push(op);
    }
    if !group.is_empty() && !(group.len() == 1 && group[0].tag == Tag::Equal) {
        groups.push(group);
    }
    groups
}

fn opcodes(a: &[&str], b: &[&str]) -> Vec<Opcode> {
    let mut i = 0;
    let mut j = 0;
    let mut out = Vec::new();
    for (ai, bj, size) in matching_blocks(a, b) {
        let tag = if i < ai && j < bj {
            Some(Tag::Replace)
        } else if i < ai {
            Some(Tag::Delete)
        } else if j < bj {
            Some(Tag::Insert)
        } else {
            None
        };
        if let Some(tag) = tag {
            out.push(Opcode {
                tag,
                i1: i,
                i2: ai,
                j1: j,
                j2: bj,
            });
        }
        i = ai + size;
        j = bj + size;
        if size > 0 {
            out.push(Opcode {
                tag: Tag::Equal,
                i1: ai,
                i2: i,
                j1: bj,
                j2: j,
            });
        }
    }
    out
}

fn matching_blocks(a: &[&str], b: &[&str]) -> Vec<(usize, usize, usize)> {
    let la = a.len();
    let lb = b.len();
    let b2j = chain_b(b);
    let mut queue = vec![(0usize, la, 0usize, lb)];
    let mut matching = Vec::new();
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = find_longest_match(a, b, &b2j, alo, ahi, blo, bhi);
        if k > 0 {
            matching.push((i, j, k));
            if alo < i && blo < j {
                queue.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                queue.push((i + k, ahi, j + k, bhi));
            }
        }
    }
    matching.sort_unstable();

    let mut non_adjacent = Vec::new();
    let mut i1 = 0;
    let mut j1 = 0;
    let mut k1 = 0;
    for (i2, j2, k2) in matching {
        if i1 + k1 == i2 && j1 + k1 == j2 {
            k1 += k2;
        } else {
            if k1 > 0 {
                non_adjacent.push((i1, j1, k1));
            }
            i1 = i2;
            j1 = j2;
            k1 = k2;
        }
    }
    if k1 > 0 {
        non_adjacent.push((i1, j1, k1));
    }
    non_adjacent.push((la, lb, 0));
    non_adjacent
}

fn chain_b<'a>(b: &[&'a str]) -> HashMap<&'a str, Vec<usize>> {
    let mut b2j: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, elt) in b.iter().enumerate() {
        b2j.entry(*elt).or_default().push(i);
    }
    let n = b.len();
    // difflib autojunk: drop elements that appear more than 1% of the time
    // once the sequence is long enough.
    if n >= 200 {
        let ntest = n / 100 + 1;
        b2j.retain(|_, idxs| idxs.len() <= ntest);
    }
    b2j
}

fn find_longest_match(
    a: &[&str],
    b: &[&str],
    b2j: &HashMap<&str, Vec<usize>>,
    alo: usize,
    ahi: usize,
    blo: usize,
    bhi: usize,
) -> (usize, usize, usize) {
    let mut besti = alo;
    let mut bestj = blo;
    let mut bestsize = 0;
    let mut j2len: HashMap<usize, usize> = HashMap::new();
    for (i, line) in a.iter().enumerate().take(ahi).skip(alo) {
        let mut newj2len = HashMap::new();
        if let Some(indexes) = b2j.get(line) {
            for &j in indexes {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let k = j2len.get(&j.wrapping_sub(1)).copied().unwrap_or(0) + 1;
                newj2len.insert(j, k);
                if k > bestsize {
                    besti = i + 1 - k;
                    bestj = j + 1 - k;
                    bestsize = k;
                }
            }
        }
        j2len = newj2len;
    }

    // Extend over popular (autojunk-purged) equal lines on either side.
    while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
        besti -= 1;
        bestj -= 1;
        bestsize += 1;
    }
    while besti + bestsize < ahi
        && bestj + bestsize < bhi
        && a[besti + bestsize] == b[bestj + bestsize]
    {
        bestsize += 1;
    }
    (besti, bestj, bestsize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_to_content_hunk_is_zero_zero() {
        let d = diff_text("", "fn f\n  return\n", "a", "b");
        assert_eq!(d, "--- a\n+++ b\n@@ -0,0 +1,2 @@\n+fn f\n+  return\n");
    }
}
