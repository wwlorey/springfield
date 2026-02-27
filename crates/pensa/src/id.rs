use uuid::Uuid;

pub fn generate_id() -> String {
    let uuid = Uuid::now_v7();
    let hex = format!("{:032x}", uuid.as_u128());
    // Use the last 8 hex chars (random_b portion of UUIDv7) for
    // collision resistance even when IDs are generated in the same millisecond.
    format!("pn-{}", &hex[24..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_is_correct() {
        let id = generate_id();
        assert!(id.starts_with("pn-"));
        assert_eq!(id.len(), 11); // "pn-" (3) + 8 hex chars
        assert!(id[3..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn ids_are_unique() {
        let a = generate_id();
        let b = generate_id();
        assert_ne!(a, b);
    }
}
