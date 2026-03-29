use std::fs;
use std::io::Write;
use std::path::Path;

use crate::error::Error;
use crate::schema::MessageEnvelope;

pub fn write_messages(path: &Path, messages: &[MessageEnvelope]) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_path = path.with_extension(format!(
        "{}tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .map(|value| format!("{value}."))
            .unwrap_or_default()
    ));

    {
        let mut file = fs::File::create(&temp_path)?;
        for message in messages {
            serde_json::to_writer(&mut file, message)?;
            file.write_all(b"\n")?;
        }
        file.sync_all()?;
    }

    fs::rename(&temp_path, path)?;
    Ok(())
}
