use std::sync::LazyLock;

use num_format::{Locale, SystemLocale, ToFormattedString};

static SYSTEM_LOCALE: LazyLock<Option<SystemLocale>> =
    LazyLock::new(|| SystemLocale::default().ok());

/// Format a count with the system locale's digit group separators, falling
/// back to en-style commas if the system locale can't be determined.
pub fn group_digits(n: usize) -> String {
    SYSTEM_LOCALE.as_ref().map_or_else(
        || n.to_formatted_string(&Locale::en),
        |locale| n.to_formatted_string(locale),
    )
}
