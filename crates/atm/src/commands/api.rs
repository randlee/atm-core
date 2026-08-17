use anyhow::Result;
use clap::{Args, Subcommand};

use crate::observability::CliObservability;

const OPENAPI_YAML: &str = include_str!("../../../../docs/atm-http-runtime/openapi.yaml");

#[derive(Debug, Args)]
pub struct ApiCommand {
    #[command(subcommand)]
    command: ApiSubcommand,
}

impl ApiCommand {
    pub fn run(self, _observability: &CliObservability) -> Result<()> {
        match self.command {
            ApiSubcommand::Spec { format } => {
                let output = match format.as_str() {
                    "yaml" => OPENAPI_YAML.to_owned(),
                    "json" => serde_json::to_string_pretty(&serde_yaml::from_str::<
                        serde_json::Value,
                    >(OPENAPI_YAML)?)?,
                    _ => unreachable!("clap constrains API spec formats"),
                };
                println!("{output}");
                Ok(())
            }
        }
    }
}

#[derive(Debug, Subcommand)]
enum ApiSubcommand {
    /// Print the versioned daemon OpenAPI contract.
    Spec {
        #[arg(long, default_value = "yaml", value_parser = ["json", "yaml"])]
        format: String,
    },
}

#[cfg(test)]
mod tests {
    use super::OPENAPI_YAML;
    use atm_core::api::endpoint_for;
    use atm_core::doctor::DoctorQuery;
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};

    #[test]
    fn openapi_contract_is_versioned_and_parseable() {
        let document: serde_json::Value = serde_yaml::from_str(OPENAPI_YAML).expect("OpenAPI YAML");
        assert_eq!(document["openapi"], "3.1.0");
        assert!(document["paths"]["/messages"].is_object());
    }

    #[test]
    fn openapi_declares_the_implemented_doctor_exchange() {
        let document: serde_json::Value = serde_yaml::from_str(OPENAPI_YAML).expect("OpenAPI YAML");
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        let (method, path) = endpoint_for(&request);
        let openapi_path = path.strip_prefix("/v1/atm").expect("API base path");

        assert!(document["paths"][openapi_path][method.to_ascii_lowercase()].is_object());
        assert!(document["paths"][openapi_path][method.to_ascii_lowercase()]["responses"]["200"]
            ["content"]["application/json"]["schema"]
            .is_object());
        assert!(
            serde_json::to_value(ResponseEnvelope::Error(
                atm_core::error::AtmError::daemon_unavailable("test"),
            ))
            .expect("error response JSON")
            .is_object()
        );
    }
}
