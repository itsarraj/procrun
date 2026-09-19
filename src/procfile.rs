#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessEntry {
    pub name: String,
    pub command: String,
}

/// Parses a `Procfile`: one `name: command` per line. The command is
/// kept as a raw string, unsplit — it's later handed to `sh -c`
/// wholesale, the same as Ruby foreman does, so pipes, `&&`, and env
/// var expansion in the command itself all keep working exactly as
/// typed rather than needing procrun to understand shell syntax.
pub fn parse_procfile(text: &str) -> Result<Vec<ProcessEntry>, String> {
    let mut entries = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, command) = line.split_once(':').ok_or_else(|| {
            format!(
                "line {}: expected 'name: command', got '{line}'",
                line_no + 1
            )
        })?;
        let name = name.trim();
        let command = command.trim();
        if name.is_empty() {
            return Err(format!("line {}: empty process name", line_no + 1));
        }
        if command.is_empty() {
            return Err(format!("line {}: '{name}' has no command", line_no + 1));
        }
        entries.push(ProcessEntry {
            name: name.to_string(),
            command: command.to_string(),
        });
    }
    if entries.is_empty() {
        return Err("Procfile has no process entries".to_string());
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_realistic_procfile() {
        let text =
            "web: cargo run --bin server\nworker: cargo run --bin worker -- --queue=default\n";
        let entries = parse_procfile(text).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "web");
        assert_eq!(entries[0].command, "cargo run --bin server");
        assert_eq!(entries[1].name, "worker");
    }

    #[test]
    fn skips_blank_lines_and_comments() {
        let text = "\n# a comment\nweb: echo hi\n\n";
        let entries = parse_procfile(text).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn a_command_containing_a_colon_is_kept_whole() {
        let text = "web: curl http://localhost:8080/health";
        let entries = parse_procfile(text).unwrap();
        assert_eq!(entries[0].command, "curl http://localhost:8080/health");
    }

    #[test]
    fn line_without_a_colon_is_a_clean_error() {
        assert!(parse_procfile("not a valid line").is_err());
    }

    #[test]
    fn empty_procfile_is_a_clean_error() {
        assert!(parse_procfile("\n# only comments\n").is_err());
    }

    #[test]
    fn empty_command_is_a_clean_error() {
        assert!(parse_procfile("web:").is_err());
    }
}
