use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Default, Clone, PartialEq, Debug)]
pub struct PickPrefs {
    pub view: String,
    pub sort: Option<(String, bool)>,
}

/// Remove an uncommitted temporary file when writing or replacing the target fails.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Read the picker settings. Missing, malformed, or incorrectly typed settings use defaults.
pub fn load(path: &Path) -> PickPrefs {
    let Ok(text) = std::fs::read_to_string(path) else {
        return PickPrefs::default();
    };
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(&text) else {
        return PickPrefs::default();
    };

    let view = object
        .get("view")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let sort = object.get("sort").and_then(|value| {
        let sort = value.as_object()?;
        Some((
            sort.get("key")?.as_str()?.to_owned(),
            sort.get("descending")?.as_bool()?,
        ))
    });

    PickPrefs { view, sort }
}

/// Save picker settings while retaining keys owned by other versions or callers.
pub fn save(path: &Path, prefs: &PickPrefs) -> io::Result<()> {
    let mut object = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();

    object.insert("view".to_owned(), Value::String(prefs.view.clone()));
    match &prefs.sort {
        Some((key, descending)) => {
            object.insert(
                "sort".to_owned(),
                serde_json::json!({"key": key, "descending": descending}),
            );
        }
        None => {
            object.remove("sort");
        }
    }
    let text = serde_json::to_vec_pretty(&Value::Object(object))?;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let (temp_path, mut temp_file) = loop {
        let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_path = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => break (temp_path, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let _temp_file = TempFile(temp_path.clone());
    temp_file.write_all(&text)?;
    drop(temp_file);
    fs::rename(&temp_path, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempFile(std::path::PathBuf);

    impl TempFile {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "rexboard-pick-prefs-{}-{nonce}.json",
                std::process::id()
            )))
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn missing_or_invalid_data_uses_defaults() {
        let file = TempFile::new();
        assert_eq!(load(&file.0), PickPrefs::default());
        std::fs::write(&file.0, "not json").unwrap();
        assert_eq!(load(&file.0), PickPrefs::default());
        std::fs::write(&file.0, "[]").unwrap();
        assert_eq!(load(&file.0), PickPrefs::default());
    }

    #[test]
    fn malformed_fields_are_ignored_independently() {
        let file = TempFile::new();
        std::fs::write(
            &file.0,
            r#"{"view":4,"sort":{"key":"name","descending":"no"}}"#,
        )
        .unwrap();
        assert_eq!(load(&file.0), PickPrefs::default());
        std::fs::write(
            &file.0,
            r#"{"view":"recent","sort":{"key":"name","descending":true}}"#,
        )
        .unwrap();
        assert_eq!(
            load(&file.0),
            PickPrefs {
                view: "recent".into(),
                sort: Some(("name".into(), true))
            }
        );
    }

    #[test]
    fn save_preserves_unknown_object_keys() {
        let file = TempFile::new();
        std::fs::write(
            &file.0,
            r#"{"future":{"enabled":true},"view":"old","other":9}"#,
        )
        .unwrap();
        let prefs = PickPrefs {
            view: "wide".into(),
            sort: Some(("field-id".into(), false)),
        };
        save(&file.0, &prefs).unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&file.0).unwrap()).unwrap();
        assert_eq!(saved["future"], serde_json::json!({"enabled":true}));
        assert_eq!(saved["other"], 9);
        assert_eq!(load(&file.0), prefs);
    }
}
