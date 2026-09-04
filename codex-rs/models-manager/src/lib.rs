pub mod cache;
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

/// Convert the client version string to a whole version string (e.g. "1.2.3-alpha.4" -> "1.2.3").
pub fn client_version_to_whole() -> String {
    codex_protocol::wire_version::wire_version_triple()
}

#[cfg(test)]
mod wire_version_tests {
    #[test]
    fn default_wire_version_covers_bundled_catalog() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../models.json")).expect("valid model catalog");
        let maximum = catalog["models"]
            .as_array()
            .expect("models array")
            .iter()
            .filter_map(|model| model["minimal_client_version"].as_str())
            .max_by_key(|version| parse_triple(version))
            .expect("catalog client version");

        assert!(
            parse_triple(codex_protocol::wire_version::DEFAULT_WIRE_VERSION)
                >= parse_triple(maximum)
        );
    }

    fn parse_triple(version: &str) -> (u64, u64, u64) {
        let mut parts = version
            .split('.')
            .map(|part| part.parse().expect("numeric version"));
        (
            parts.next().expect("major"),
            parts.next().expect("minor"),
            parts.next().expect("patch"),
        )
    }
}
