//! Coder workspace names: 1-32 characters of `[a-z0-9]` in hyphen-separated runs.
//! Coder also accepts uppercase, but lowercase keeps names stable as SSH hosts.

pub(crate) const LIMIT: usize = 32;

pub(crate) fn valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= LIMIT
        && name.split('-').all(|run| {
            !run.is_empty() && run.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9'))
        })
}

/// A suggested name for a new workspace: the prefix, then the label slugged.
pub(crate) fn suggest(prefix: &str, label: &str) -> String {
    let mut name = prefix.to_owned();
    let mut pending = true;
    for c in label.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            if pending {
                name.push('-');
                pending = false;
            }
            name.push(c);
        } else {
            pending = true;
        }
        if name.len() >= LIMIT {
            break;
        }
    }
    name.truncate(LIMIT);
    name.trim_end_matches('-').to_owned()
}

#[cfg(test)]
mod tests;
