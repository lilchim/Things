use std::fmt;

/// Number of hex characters in the short ID suffix.
const SHORT_LEN: usize = 4;

/// Identifies an entity. On disk it is the folder name: `<slug>-<short>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId {
    slug: String,
    short: String,
}

#[derive(Debug, thiserror::Error)]
#[error("`{0}` is not an entity folder name (expected `slug-xxxx`)")]
pub struct ParseIdError(String);

impl EntityId {
    /// Makes a fresh ID for a new entity called `name`.
    pub fn generate(name: &str) -> Self {
        let short = (0..SHORT_LEN)
            .map(|_| char::from_digit(fastrand::u32(0..16), 16).unwrap())
            .collect();
        Self {
            slug: slugify(name),
            short,
        }
    }

    /// Parses a folder name such as `mass-effect-7f3a`.
    pub fn parse(folder: &str) -> Result<Self, ParseIdError> {
        let err = || ParseIdError(folder.to_owned());
        let (slug, short) = folder.rsplit_once('-').ok_or_else(err)?;
        let short_ok = short.len() == SHORT_LEN
            && short
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, 'a'..='f'));
        if slug.is_empty() || !short_ok || slugify(slug) != slug {
            return Err(err());
        }
        Ok(Self {
            slug: slug.to_owned(),
            short: short.to_owned(),
        })
    }

    pub fn slug(&self) -> &str {
        &self.slug
    }

    pub fn short(&self) -> &str {
        &self.short
    }

    /// A display name guessed from the slug: `mass-effect-2` -> `Mass Effect 2`.
    pub fn title(&self) -> String {
        title_case(&self.slug)
    }
}

/// `mass-effect-2` -> `Mass Effect 2`.
pub(crate) fn title_case(slug: &str) -> String {
    slug.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.slug, self.short)
    }
}

/// Lowercase ASCII words joined by single dashes. Falls back to `thing`.
pub(crate) fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug.push_str("thing");
    }
    slug
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_round_trip() {
        let id = EntityId::generate("Mass Effect 2: Collector's Edition");
        assert_eq!(id.slug(), "mass-effect-2-collector-s-edition");
        assert_eq!(EntityId::parse(&id.to_string()).unwrap(), id);
    }

    #[test]
    fn parse_rejects_non_entity_names() {
        for bad in [
            "mass-effect",
            "-7f3a",
            "Mass-7f3a",
            "mass-7f3",
            "mass-7g3a",
            ".git",
        ] {
            assert!(EntityId::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn title_from_slug() {
        let id = EntityId::parse("mass-effect-2-7f3a").unwrap();
        assert_eq!(id.title(), "Mass Effect 2");
    }

    #[test]
    fn empty_names_still_get_a_slug() {
        assert_eq!(EntityId::generate("???").slug(), "thing");
    }
}
