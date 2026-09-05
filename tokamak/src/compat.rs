use std::fs;
use std::io;
use std::path::Path;

use crate::{builtins, events, globals, network, streams};

const SOURCES: &[(&str, &str)] = &[
    ("builtins/cloudflare-workers.mjs", builtins::SOURCE),
    ("events/events.mjs", events::SOURCE),
    ("globals/console.mjs", globals::CONSOLE_SOURCE),
    ("globals/process.mjs", globals::PROCESS_SOURCE),
    ("globals/web.mjs", globals::WEB_SOURCE),
    ("network/fetch.mjs", network::FETCH_SOURCE),
    ("network/url.mjs", network::URL_SOURCE),
    ("network/websocket.mjs", network::WEBSOCKET_SOURCE),
    ("streams/node.mjs", streams::NODE_SOURCE),
    ("streams/text.mjs", streams::TEXT_SOURCE),
    ("streams/web.mjs", streams::WEB_SOURCE),
];

/// Write the JavaScript compatibility sources used while bundling a Worker.
///
/// # Errors
///
/// Returns an error when a source directory or file cannot be created.
pub fn write_worker_compatibility_sources(destination: impl AsRef<Path>) -> io::Result<()> {
    let destination = destination.as_ref();
    for (relative_path, source) in SOURCES {
        let path = destination.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, source)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{SOURCES, write_worker_compatibility_sources};

    #[test]
    fn writes_each_feature_source_at_its_module_path() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        write_worker_compatibility_sources(directory.path())?;

        for (relative_path, source) in SOURCES {
            assert_eq!(
                fs::read_to_string(directory.path().join(relative_path))?,
                *source
            );
        }
        Ok(())
    }
}
