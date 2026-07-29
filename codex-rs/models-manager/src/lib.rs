pub(crate) mod cache;
pub mod collaboration_mode_presets;
pub(crate) mod config;
pub mod manager;
pub mod model_info;
pub mod model_presets;
pub mod test_support;

pub use codex_protocol::auth::AuthMode;
pub use config::ModelsManagerConfig;

/// Load the bundled model catalog shipped with `codex-models-manager`.
pub fn bundled_models_response()
-> std::result::Result<codex_protocol::openai_models::ModelsResponse, serde_json::Error> {
    serde_json::from_str(include_str!("../models.json"))
}

/// Whole (`MAJOR.MINOR.PATCH`) client version sent to the backend as the `client_version`
/// query parameter on `/backend-api/codex/models`, and used to key models-cache freshness.
///
/// This intentionally reports the backend *compatibility* version rather than this fork's
/// package version: the backend returns an empty model list for `0.1.0`. See
/// `codex_agent_identity::wire_compat_version_triple`. `codex --version`, npm package
/// versions, and release tags are unaffected.
pub fn client_version_to_whole() -> String {
    codex_agent_identity::wire_compat_version_triple()
}

#[cfg(test)]
mod client_version_tests {
    use super::client_version_to_whole;
    use codex_agent_identity::DEFAULT_WIRE_COMPAT_VERSION_TRIPLE;
    use codex_agent_identity::WIRE_VERSION_OVERRIDE_ENV_VAR;
    use codex_agent_identity::wire_compat_version_triple;

    /// The wiring test: whatever the override resolves to, `/models` must send it.
    /// Override resolution itself (including malformed-value fallback) is covered by
    /// `codex-agent-identity`, which owns the env var and serializes those tests.
    #[test]
    fn client_version_to_whole_delegates_to_wire_compat_version() {
        assert_eq!(client_version_to_whole(), wire_compat_version_triple());
    }

    /// Regression test for the model gate: the backend returns an empty model list for the
    /// fork's package version (`0.1.0`), so `client_version` must be the wire-compat
    /// version, not `CARGO_PKG_VERSION_{MAJOR,MINOR,PATCH}`.
    #[test]
    fn client_version_to_whole_is_the_wire_version_not_the_package_version() {
        if std::env::var_os(WIRE_VERSION_OVERRIDE_ENV_VAR).is_some() {
            // An ambient override is in effect; the delegation test above still applies.
            return;
        }
        let package_triple = format!(
            "{}.{}.{}",
            env!("CARGO_PKG_VERSION_MAJOR"),
            env!("CARGO_PKG_VERSION_MINOR"),
            env!("CARGO_PKG_VERSION_PATCH")
        );
        assert_eq!(
            client_version_to_whole(),
            DEFAULT_WIRE_COMPAT_VERSION_TRIPLE
        );
        assert_ne!(
            client_version_to_whole(),
            package_triple,
            "fork package version must not be sent as the /models client_version"
        );
    }

    /// Drift guard for the hardcoded backend compatibility version.
    ///
    /// `DEFAULT_WIRE_COMPAT_VERSION` is a literal, so it will silently go stale as upstream
    /// moves. The bundled catalog carries each model's `minimal_client_version` floor, which
    /// upstream raises when it ships a model that needs a newer client. If our advertised
    /// version ever falls below the highest bundled floor, the backend will start answering
    /// with an empty model list and a 400 on `/responses` again. Fail here instead, on the
    /// upstream merge that introduces the drift.
    #[test]
    fn default_wire_compat_version_is_not_behind_the_bundled_model_floors() {
        fn parse_triple(value: &serde_json::Value) -> Option<(u64, u64, u64)> {
            match value {
                // The bundled catalog uses "0.144.0"; the live API has used [0, 144, 0].
                serde_json::Value::String(text) => {
                    let mut parts = text.split('.');
                    let major = parts.next()?.parse().ok()?;
                    let minor = parts.next()?.parse().ok()?;
                    let patch = parts.next()?.parse().ok()?;
                    parts.next().is_none().then_some((major, minor, patch))
                }
                serde_json::Value::Array(items) if items.len() == 3 => {
                    Some((items[0].as_u64()?, items[1].as_u64()?, items[2].as_u64()?))
                }
                _ => None,
            }
        }

        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../models.json")).expect("bundled catalog parses");
        let models = catalog["models"]
            .as_array()
            .expect("bundled catalog has a models array");

        let highest_floor = models
            .iter()
            .filter_map(|model| model.get("minimal_client_version"))
            .map(|value| {
                parse_triple(value)
                    .unwrap_or_else(|| panic!("unparseable minimal_client_version: {value}"))
            })
            .max()
            .expect("bundled catalog declares at least one minimal_client_version");

        let advertised = parse_triple(&serde_json::Value::String(
            DEFAULT_WIRE_COMPAT_VERSION_TRIPLE.to_string(),
        ))
        .expect("DEFAULT_WIRE_COMPAT_VERSION_TRIPLE is a numeric triple");

        assert!(
            advertised >= highest_floor,
            "DEFAULT_WIRE_COMPAT_VERSION ({DEFAULT_WIRE_COMPAT_VERSION_TRIPLE}) is behind the highest bundled minimal_client_version ({}.{}.{}); bump it or the backend will return an empty model list again",
            highest_floor.0,
            highest_floor.1,
            highest_floor.2,
        );
    }
}
