use sc_observability_log::instrument;

#[instrument(unsupported_option = true)]
fn rejected() {}

fn main() { rejected(); }
