//! Stable hunk identifiers: hash of file path + hunk body, excluding line numbers.

use sha2::{Digest, Sha256};
use std::collections::HashMap;

use super::diff::DiffHunk;

fn raw_id(hunk: &DiffHunk) -> String {
    let mut hasher = Sha256::new();
    hasher.update(hunk.file.as_bytes());
    for line in &hunk.lines {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    digest[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()[..7]
        .to_string()
}

/// Assign unique IDs. Hunks with identical content get `-2`, `-3`... suffixes.
pub fn assign_ids(hunks: &[DiffHunk]) -> Vec<(String, &DiffHunk)> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    hunks
        .iter()
        .map(|hunk| {
            let raw = raw_id(hunk);
            let count = seen.entry(raw.clone()).or_insert(0);
            *count += 1;
            let id = if *count == 1 {
                raw
            } else {
                format!("{raw}-{count}")
            };
            (id, hunk)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunk(file: &str, header: &str, lines: &[&str]) -> DiffHunk {
        DiffHunk {
            file: file.into(),
            file_header: String::new(),
            header: header.into(),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            unsupported_metadata: None,
        }
    }

    #[test]
    fn id_ignores_line_numbers() {
        let a = hunk("f", "@@ -1 +1 @@", &["-a", "+b"]);
        let b = hunk("f", "@@ -50 +50 @@", &["-a", "+b"]);
        assert_eq!(assign_ids(&[a])[0].0, assign_ids(&[b])[0].0);
    }

    #[test]
    fn duplicates_get_suffix() {
        let a = hunk("f", "@@ -1 +1 @@", &["-a", "+b"]);
        let b = hunk("f", "@@ -9 +9 @@", &["-a", "+b"]);
        let hunks = [a, b];
        let ids = assign_ids(&hunks);
        assert_eq!(ids[1].0, format!("{}-2", ids[0].0));
        assert_eq!(ids[0].0.len(), 7);
    }

    #[test]
    fn different_files_differ() {
        let a = hunk("f", "@@ -1 +1 @@", &["-a", "+b"]);
        let b = hunk("g", "@@ -1 +1 @@", &["-a", "+b"]);
        let hunks = [a, b];
        let ids = assign_ids(&hunks);
        assert_ne!(ids[0].0, ids[1].0);
    }
}
