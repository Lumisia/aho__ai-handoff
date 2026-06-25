/// A plan for a single file's changes.
pub struct FilePlan {
    pub path: String,
    pub before: Option<String>,
    pub after: String,
}

/// Renders a list of FilePlans as a human-readable dry-run diff.
///
/// For each file:
/// - If `before` is `None`, shows `CREATE <path>`
/// - Otherwise, shows `MODIFY <path>`
/// - Followed by a minimal line diff:
///   - Lines present in `after` but not in `before` are prefixed with `+`
///   - Lines present in `before` but not in `after` are prefixed with `-`
///   - Order of lines in the output matches their order in the source files
///
/// The output includes the `task_note` (e.g., in a header or footer).
pub fn render(plans: &[FilePlan], task_note: &str) -> String {
    let mut output = String::new();

    // Add task note as a header
    output.push_str(&format!("Task: {}\n\n", task_note));

    for plan in plans {
        // Determine operation type
        if plan.before.is_none() {
            output.push_str(&format!("CREATE {}\n", plan.path));
        } else {
            output.push_str(&format!("MODIFY {}\n", plan.path));
        }

        // Split into lines and compute diff
        let before_lines: Vec<&str> = plan
            .before
            .as_ref()
            .map(|s| s.lines().collect())
            .unwrap_or_default();
        let after_lines: Vec<&str> = plan.after.lines().collect();

        // For CREATE, all lines are additions
        if plan.before.is_none() {
            for line in &after_lines {
                output.push_str(&format!("+{}\n", line));
            }
        } else {
            // For MODIFY, compute line set difference while preserving order
            let before_set: std::collections::HashSet<_> = before_lines.iter().copied().collect();
            let after_set: std::collections::HashSet<_> = after_lines.iter().copied().collect();

            // Show removed lines (in before_set but not after_set) in order they appear in before
            for line in &before_lines {
                if !after_set.contains(line) {
                    output.push_str(&format!("-{}\n", line));
                }
            }

            // Show added lines (in after_set but not before_set) in order they appear in after
            for line in &after_lines {
                if !before_set.contains(line) {
                    output.push_str(&format!("+{}\n", line));
                }
            }
        }

        output.push('\n');
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_plan_contains_header_and_plus_lines() {
        let plan = FilePlan {
            path: "new_file.txt".to_string(),
            before: None,
            after: "line 1\nline 2\nline 3".to_string(),
        };
        let output = render(&[plan], "test task");

        assert!(output.contains("CREATE new_file.txt"));
        assert!(output.contains("+line 1"));
        assert!(output.contains("+line 2"));
        assert!(output.contains("+line 3"));
        assert!(output.contains("Task: test task"));
    }

    #[test]
    fn modify_plan_with_one_added_line() {
        let plan = FilePlan {
            path: "existing_file.txt".to_string(),
            before: Some("line 1\nline 2".to_string()),
            after: "line 1\nline 2\nline 3".to_string(),
        };
        let output = render(&[plan], "test task");

        assert!(output.contains("MODIFY existing_file.txt"));
        assert!(output.contains("+line 3"));
        assert!(!output.contains("-line 1"));
        assert!(!output.contains("-line 2"));
        assert!(output.contains("Task: test task"));
    }

    #[test]
    fn task_note_appears_in_output() {
        let plan = FilePlan {
            path: "file.txt".to_string(),
            before: Some("old content".to_string()),
            after: "new content".to_string(),
        };
        let output = render(&[plan], "my special note");

        assert!(output.contains("Task: my special note"));
    }
}
