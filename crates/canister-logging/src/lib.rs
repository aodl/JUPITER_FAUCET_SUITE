pub const FIELD_EVENT: &str = "event";
pub const FIELD_TIMERS_INSTALLED: &str = "timers_installed";
pub const FIELD_MAIN_INTERVAL_SECONDS: &str = "main_interval_seconds";

const MAX_FIELD_VALUE_BYTES: usize = 256;

pub fn format_event_line(canister: &str, event: &str, fields: &[(&str, String)]) -> String {
    let mut line = String::new();
    line.push_str(&escape_value(canister));
    line.push(' ');
    line.push_str(&escape_value(event));
    for (key, value) in fields {
        line.push(' ');
        line.push_str(&escape_key(key));
        line.push('=');
        line.push_str(&escape_bounded_value(value, MAX_FIELD_VALUE_BYTES));
    }
    line
}

pub fn escape_value(value: &str) -> String {
    escape_bounded_value(value, usize::MAX)
}

pub fn escape_bounded_value(value: &str, max_bytes: usize) -> String {
    let mut out = String::new();
    for (idx, byte) in value.bytes().enumerate() {
        if idx >= max_bytes {
            out.push_str("%E2%80%A6");
            break;
        }
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' | b':' => {
                out.push(byte as char)
            }
            _ => {
                out.push('%');
                out.push(hex_digit(byte >> 4));
                out.push(hex_digit(byte & 0x0f));
            }
        }
    }
    out
}

fn escape_key(key: &str) -> String {
    key.bytes()
        .filter_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' => Some(byte as char),
            _ => None,
        })
        .collect()
}

fn hex_digit(nibble: u8) -> char {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    HEX[nibble as usize] as char
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_fields_in_given_order() {
        let line = format_event_line(
            "relay",
            "LIFECYCLE",
            &[
                (FIELD_EVENT, "post_upgrade_complete".to_string()),
                (FIELD_TIMERS_INSTALLED, true.to_string()),
                (FIELD_MAIN_INTERVAL_SECONDS, 86_400_u64.to_string()),
            ],
        );
        assert_eq!(
            line,
            "relay LIFECYCLE event=post_upgrade_complete timers_installed=true main_interval_seconds=86400"
        );
    }

    #[test]
    fn escapes_values_and_removes_key_noise() {
        let line = format_event_line(
            "relay\nbad",
            "ADMIN",
            &[("bad-key", "line one\nline two value".to_string())],
        );
        assert_eq!(
            line,
            "relay%0Abad ADMIN badkey=line%20one%0Aline%20two%20value"
        );
        assert!(!line.contains('\n'));
    }

    #[test]
    fn handles_empty_fields() {
        assert_eq!(
            format_event_line("lifeline", "LIFECYCLE", &[]),
            "lifeline LIFECYCLE"
        );
    }

    #[test]
    fn bounds_long_values() {
        let long = "a".repeat(MAX_FIELD_VALUE_BYTES + 1);
        let line = format_event_line("relay", "ERR", &[("message", long)]);
        assert!(line.ends_with("%E2%80%A6"));
    }
}
