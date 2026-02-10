//! Email address parsing (RFC 5322 §3.4).

/// A parsed email address.
///
/// # Examples
/// - `"Juan García <juan@ejemplo.com>"` → `display_name = "Juan García"`, `address = "juan@ejemplo.com"`
/// - `"user@example.com"` → `display_name = ""`, `address = "user@example.com"`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct EmailAddress {
    /// Human-readable display name (may be empty).
    pub display_name: String,
    /// The bare email address (`user@domain`).
    pub address: String,
}

impl EmailAddress {
    /// Parse a single email address from a header value.
    ///
    /// Supported formats:
    /// - `"user@domain.com"`
    /// - `"<user@domain.com>"`
    /// - `"Display Name <user@domain.com>"`
    /// - `"\"Display, Name\" <user@domain.com>"`
    ///
    /// If parsing fails, the raw string is stored as `address`.
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Self {
                display_name: String::new(),
                address: String::new(),
            };
        }

        // Try "Display Name <address>" or "<address>"
        if let Some(angle_start) = trimmed.rfind('<') {
            if let Some(angle_end) = trimmed.rfind('>') {
                if angle_end > angle_start {
                    let addr = trimmed[angle_start + 1..angle_end].trim().to_string();
                    let name_part = trimmed[..angle_start].trim();
                    let display_name = strip_quotes(name_part);
                    return Self {
                        display_name,
                        address: addr,
                    };
                }
            }
        }

        // Bare address: "user@domain.com"
        if trimmed.contains('@') {
            return Self {
                display_name: String::new(),
                address: trimmed.to_string(),
            };
        }

        // Fallback: store as-is
        Self {
            display_name: String::new(),
            address: trimmed.to_string(),
        }
    }

    /// Parse a comma-separated list of addresses.
    ///
    /// Handles quoted commas: `"Last, First" <a@b.com>, other@c.com`
    pub fn parse_list(raw: &str) -> Vec<Self> {
        let mut results = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut in_angle = false;

        for ch in raw.chars() {
            match ch {
                '"' => {
                    in_quotes = !in_quotes;
                    current.push(ch);
                }
                '<' if !in_quotes => {
                    in_angle = true;
                    current.push(ch);
                }
                '>' if !in_quotes => {
                    in_angle = false;
                    current.push(ch);
                }
                ',' if !in_quotes && !in_angle => {
                    let addr = Self::parse(&current);
                    if !addr.address.is_empty() {
                        results.push(addr);
                    }
                    current.clear();
                }
                _ => current.push(ch),
            }
        }

        // Last segment
        let addr = Self::parse(&current);
        if !addr.address.is_empty() {
            results.push(addr);
        }

        results
    }

    /// Format for display: `"Display Name <address>"` or just `"address"`.
    pub fn display(&self) -> String {
        if self.display_name.is_empty() {
            self.address.clone()
        } else {
            format!("{} <{}>", self.display_name, self.address)
        }
    }
}

/// Strip surrounding double-quotes and trim whitespace.
fn strip_quotes(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        trimmed[1..trimmed.len() - 1].trim().to_string()
    } else {
        trimmed.to_string()
    }
}

impl std::fmt::Display for EmailAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bare_address() {
        let addr = EmailAddress::parse("user@example.com");
        assert_eq!(addr.address, "user@example.com");
        assert_eq!(addr.display_name, "");
    }

    #[test]
    fn test_parse_angle_address() {
        let addr = EmailAddress::parse("<user@example.com>");
        assert_eq!(addr.address, "user@example.com");
        assert_eq!(addr.display_name, "");
    }

    #[test]
    fn test_parse_name_and_address() {
        let addr = EmailAddress::parse("User One <user1@example.com>");
        assert_eq!(addr.address, "user1@example.com");
        assert_eq!(addr.display_name, "User One");
    }

    #[test]
    fn test_parse_quoted_name() {
        let addr = EmailAddress::parse("\"Last, First\" <user@example.com>");
        assert_eq!(addr.address, "user@example.com");
        assert_eq!(addr.display_name, "Last, First");
    }

    #[test]
    fn test_parse_list() {
        let list =
            EmailAddress::parse_list("User One <a@b.com>, User Two <c@d.com>, plain@addr.com");
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].address, "a@b.com");
        assert_eq!(list[1].display_name, "User Two");
        assert_eq!(list[2].address, "plain@addr.com");
    }

    #[test]
    fn test_parse_list_with_quoted_comma() {
        let list = EmailAddress::parse_list("\"Last, First\" <a@b.com>, other@c.com");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].display_name, "Last, First");
        assert_eq!(list[0].address, "a@b.com");
    }

    #[test]
    fn test_display_with_name() {
        let addr = EmailAddress {
            display_name: "Alice".to_string(),
            address: "alice@example.com".to_string(),
        };
        assert_eq!(addr.display(), "Alice <alice@example.com>");
    }

    #[test]
    fn test_display_without_name() {
        let addr = EmailAddress {
            display_name: String::new(),
            address: "alice@example.com".to_string(),
        };
        assert_eq!(addr.display(), "alice@example.com");
    }

    #[test]
    fn test_parse_empty() {
        let addr = EmailAddress::parse("");
        assert_eq!(addr.address, "");
    }
}
