use std::collections::HashMap;

/// A deliberately minimal `.env` parser: `KEY=value` per line, `#`
/// comments, blank lines skipped, and one layer of surrounding quotes
/// (`"..."` or `'...'`) stripped if present. No variable expansion, no
/// multi-line values, no escape sequences — real `.env` files in the
/// wild are almost always this simple, and procrun's job is running
/// processes, not being a second dotenv parser to keep in sync with
/// every edge case of the many competing `.env` conventions.
pub fn parse_dotenv(text: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let mut value = value.trim();
        if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
            || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
        {
            value = &value[1..value.len() - 1];
        }
        if !key.is_empty() {
            vars.insert(key.to_string(), value.to_string());
        }
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_key_value_pairs() {
        let vars = parse_dotenv("PORT=3000\nDEBUG=true\n");
        assert_eq!(vars.get("PORT"), Some(&"3000".to_string()));
        assert_eq!(vars.get("DEBUG"), Some(&"true".to_string()));
    }

    #[test]
    fn strips_one_layer_of_surrounding_quotes() {
        let vars = parse_dotenv("NAME=\"hello world\"\nSINGLE='quoted'\n");
        assert_eq!(vars.get("NAME"), Some(&"hello world".to_string()));
        assert_eq!(vars.get("SINGLE"), Some(&"quoted".to_string()));
    }

    #[test]
    fn skips_blank_lines_and_comments() {
        let vars = parse_dotenv("\n# a comment\nKEY=value\n");
        assert_eq!(vars.len(), 1);
    }

    #[test]
    fn a_value_containing_an_equals_sign_is_kept_whole() {
        let vars = parse_dotenv("DATABASE_URL=postgres://u:p@host/db?sslmode=require");
        assert_eq!(
            vars.get("DATABASE_URL"),
            Some(&"postgres://u:p@host/db?sslmode=require".to_string())
        );
    }

    #[test]
    fn a_line_without_an_equals_sign_is_skipped_not_an_error() {
        let vars = parse_dotenv("not a valid line\nKEY=value\n");
        assert_eq!(vars.len(), 1);
    }
}
