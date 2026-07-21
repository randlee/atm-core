use anyhow::Result;
use clap::{Args, Subcommand};

use crate::observability::CliObservability;

const OPENAPI_YAML: &str = include_str!("../../../../docs/atm-daemon/openapi.yaml");

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

    #[test]
    fn openapi_contract_is_versioned_and_parseable() {
        let document: serde_json::Value = serde_yaml::from_str(OPENAPI_YAML).expect("OpenAPI YAML");
        assert_eq!(document["openapi"], "3.1.0");
        assert!(document["paths"]["/messages"].is_object());
    }
}
