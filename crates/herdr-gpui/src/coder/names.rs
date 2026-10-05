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

const ADJECTIVES: [&str; 32] = [
    "amber", "bold", "brave", "bright", "calm", "clever", "cosmic", "crisp", "eager", "fancy",
    "gentle", "glad", "golden", "happy", "jolly", "keen", "lively", "lucky", "mellow", "misty",
    "nimble", "noble", "quiet", "rapid", "rosy", "silent", "sunny", "swift", "tidy", "vivid",
    "witty", "zesty",
];

const NOUNS: [&str; 32] = [
    "badger", "beacon", "breeze", "canyon", "cedar", "comet", "coral", "falcon", "fern", "fjord",
    "galaxy", "harbor", "heron", "island", "lagoon", "lark", "maple", "meadow", "nebula", "otter",
    "pebble", "pine", "quartz", "raven", "reef", "river", "sparrow", "summit", "tundra", "valley",
    "willow", "zephyr",
];

/// A fresh name for a new workspace, `prefix-adjective-noun`, with a short
/// random suffix when it fits; always valid. Randomness comes from the OS,
/// falling back to the clock, which only makes a clash slightly likelier.
pub(crate) fn random(prefix: &str) -> String {
    let mut bytes = [0u8; 3];
    if getrandom::fill(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.subsec_nanos())
            .unwrap_or_default();
        bytes = [(nanos >> 16) as u8, (nanos >> 8) as u8, nanos as u8];
    }
    let adjective = ADJECTIVES[usize::from(bytes[0]) % ADJECTIVES.len()];
    let noun = NOUNS[usize::from(bytes[1]) % NOUNS.len()];
    let base = format!("{prefix}-{adjective}-{noun}");
    let suffixed = format!("{base}-{:02x}", bytes[2]);
    let name = if suffixed.len() <= LIMIT {
        suffixed
    } else {
        base
    };
    if valid(&name) {
        name
    } else {
        suggest(prefix, &format!("{adjective} {noun}"))
    }
}

#[cfg(test)]
mod tests;
