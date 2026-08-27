//! Serialized Worker environment bindings.

use std::collections::BTreeMap;
use std::fs;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app_layout::AppLayout;
use crate::worker_package_contract::Result;

/// Bindings that appd passes to a Worker at runtime.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkerEnvironment {
    /// Text and JSON values declared in Wrangler's `vars` section.
    #[serde(default)]
    pub vars: BTreeMap<String, Value>,
}

/// Write the normalized Worker environment into an app bundle.
///
/// # Errors
///
/// Returns an error when the environment cannot be serialized or written.
pub fn write(layout: &AppLayout, environment: &WorkerEnvironment) -> Result<()> {
    fs::write(
        layout.worker_environment(),
        serde_json::to_vec(environment)?,
    )?;
    Ok(())
}

/// Read the normalized Worker environment from an app bundle.
///
/// # Errors
///
/// Returns an error when the environment cannot be read or parsed.
pub fn load(layout: &AppLayout) -> Result<WorkerEnvironment> {
    Ok(serde_json::from_slice(&fs::read(
        layout.worker_environment(),
    )?)?)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::{WorkerEnvironment, load, write};
    use crate::app_layout::AppLayout;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn round_trips_text_and_json_vars() -> TestResult {
        let directory = tempfile::tempdir()?;
        let layout = AppLayout::new(directory.path());
        let environment = WorkerEnvironment {
            vars: BTreeMap::from([
                ("TEXT".to_owned(), json!("value")),
                ("JSON".to_owned(), json!({ "enabled": true })),
            ]),
        };

        write(&layout, &environment)?;

        assert_eq!(load(&layout)?, environment);
        Ok(())
    }
}
