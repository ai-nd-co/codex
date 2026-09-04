use std::borrow::Cow;

pub const DEFAULT_WIRE_VERSION: &str = "0.153.0";
pub const WIRE_VERSION_OVERRIDE_ENV_VAR: &str = "CODEX_WIRE_VERSION_OVERRIDE";

pub fn wire_version() -> Cow<'static, str> {
    resolve_wire_version(std::env::var(WIRE_VERSION_OVERRIDE_ENV_VAR).ok().as_deref())
}

pub fn wire_version_triple() -> String {
    version_triple(&wire_version()).unwrap_or_else(|| DEFAULT_WIRE_VERSION.to_string())
}

fn resolve_wire_version(value: Option<&str>) -> Cow<'static, str> {
    value
        .map(str::trim)
        .filter(|value| version_triple(value).is_some())
        .map(|value| Cow::Owned(value.to_string()))
        .unwrap_or(Cow::Borrowed(DEFAULT_WIRE_VERSION))
}

fn version_triple(value: &str) -> Option<String> {
    let (without_build, build) = value
        .split_once('+')
        .map_or((value, None), |(version, build)| (version, Some(build)));
    if build
        .is_some_and(|build| !valid_identifiers(build, /*reject_numeric_leading_zero*/ false))
    {
        return None;
    }
    let (triple, prerelease) = without_build
        .split_once('-')
        .map_or((without_build, None), |(triple, prerelease)| {
            (triple, Some(prerelease))
        });
    if prerelease.is_some_and(|prerelease| {
        !valid_identifiers(prerelease, /*reject_numeric_leading_zero*/ true)
    }) {
        return None;
    }
    let parts = triple.split('.').collect::<Vec<_>>();
    (parts.len() == 3 && parts.iter().all(|part| valid_numeric_identifier(part)))
        .then(|| triple.to_string())
}

fn valid_identifiers(value: &str, reject_numeric_leading_zero: bool) -> bool {
    !value.is_empty()
        && value.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && !(reject_numeric_leading_zero
                    && identifier.bytes().all(|byte| byte.is_ascii_digit())
                    && identifier.len() > 1
                    && identifier.starts_with('0'))
        })
}

fn valid_numeric_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_must_be_a_header_safe_semver_and_is_trimmed() {
        assert_eq!(
            resolve_wire_version(Some(" 0.200.1-beta.2 ")),
            "0.200.1-beta.2"
        );
        for invalid in [
            "",
            "1.2",
            "1.2.x",
            "1..3",
            "01.2.3",
            "1.2.3-",
            "1.2.3+",
            "1.2.3-beta_1",
            "1.2.3-01",
            "1.2.3+build+extra",
            "1.2.3\nbad",
        ] {
            assert_eq!(resolve_wire_version(Some(invalid)), DEFAULT_WIRE_VERSION);
        }
    }

    #[test]
    fn triple_strips_prerelease_and_build_metadata() {
        assert_eq!(
            version_triple("1.2.3-beta.4+build.5").as_deref(),
            Some("1.2.3")
        );
    }
}
