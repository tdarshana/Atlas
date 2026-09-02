use crate::db::Db;
use crate::memories::MemoryRepo;
use crate::{AtlasError, Result};
use duckdb::params;
use serde_json::{Map, Value};

/// The only keys the settings table accepts. `set_many` rejects anything else.
pub const SETTING_KEYS: &[&str] = &[
    "extraction.enabled",
    "extraction.base_url",
    "extraction.api_key",
    "extraction.model",
    "extraction.auto_accept_min_confidence",
    "embedding.model",
    "daemon.port",
];

const API_KEY: &str = "extraction.api_key";
const MASKED: &str = "***";

/// Rejects a value whose JSON type does not match the key. Without this a client could
/// store, say, an object under `extraction.api_key`, and the extraction worker would then
/// read a value it cannot use out of the database. `Value::Null` means "unset" and is
/// accepted for every key, so the map `get_all` returns can be sent straight back.
fn check_type(key: &str, value: &Value) -> Result<()> {
    let wrong = |want: &str| Err(AtlasError::Invalid(format!("setting '{key}' must be {want}")));
    if value.is_null() {
        return Ok(());
    }
    match key {
        "extraction.api_key" | "extraction.base_url" | "extraction.model" | "embedding.model" => {
            if !value.is_string() {
                return wrong("a string");
            }
        }
        "extraction.enabled" => {
            if !value.is_boolean() {
                return wrong("a boolean");
            }
        }
        "extraction.auto_accept_min_confidence" => match value.as_f64() {
            Some(n) if (0.0..=1.0).contains(&n) => {}
            Some(_) => return wrong("a number between 0 and 1"),
            None => return wrong("a number between 0 and 1"),
        },
        "daemon.port" => match value.as_u64() {
            Some(n) if (1..=65535).contains(&n) => {}
            _ => return wrong("a port number between 1 and 65535"),
        },
        _ => {}
    }
    Ok(())
}

pub struct SettingsRepo<'a> {
    db: &'a Db,
}

impl<'a> SettingsRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// The unmasked value for a single key, for the extraction worker. `None` when unset.
    pub fn get_raw(&self, key: &str) -> Result<Option<Value>> {
        self.db.with_conn(|c| {
            let mut st = c.prepare("select value::text from settings where key = ?")?;
            let mut rows = st.query(params![key])?;
            match rows.next()? {
                Some(r) => {
                    let text: String = r.get(0)?;
                    Ok(Some(serde_json::from_str(&text)?))
                }
                None => Ok(None),
            }
        })
    }

    /// Every known key, present even when unset (as `Value::Null`); `extraction.api_key`
    /// is masked to `"***"` when it holds a non-empty value.
    pub fn get_all(&self) -> Result<Map<String, Value>> {
        let mut out = Map::new();
        for key in SETTING_KEYS {
            out.insert((*key).to_string(), self.get_raw(key)?.unwrap_or(Value::Null));
        }
        if out.get(API_KEY).and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty()) {
            out.insert(API_KEY.to_string(), Value::String(MASKED.to_string()));
        }
        Ok(out)
    }

    /// Upserts each given key. Rejects the whole call with `Invalid` if any key is
    /// unknown or holds a value of the wrong type, before writing anything. Ignores
    /// `extraction.api_key == "***"` so a masked value read back from `get_all` and
    /// sent straight back does not clobber the real key.
    pub fn set_many(&self, values: &Map<String, Value>, actor: &str) -> Result<()> {
        for key in values.keys() {
            if !SETTING_KEYS.contains(&key.as_str()) {
                return Err(AtlasError::Invalid(format!("unknown setting key '{key}'")));
            }
        }
        for (key, value) in values {
            check_type(key, value)?;
        }
        for (key, value) in values {
            if key == API_KEY && value.as_str() == Some(MASKED) {
                continue;
            }
            let json = value.to_string();
            self.db.with_conn(|c| {
                c.execute("delete from settings where key = ?", params![key])?;
                c.execute("insert into settings (key, value) values (?, ?::json)", params![key, json])?;
                Ok(())
            })?;
            // The api key value itself never goes into the audit log.
            let detail = if key == API_KEY { serde_json::json!({"key": key}) } else { serde_json::json!({"key": key, "value": value}) };
            MemoryRepo::new(self.db).audit(actor, "set", "setting", None, detail)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_keys_are_rejected_before_any_write() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        let values = Map::from_iter([("bogus".to_string(), Value::from(1))]);
        let err = repo.set_many(&values, "t").unwrap_err();
        assert!(matches!(err, AtlasError::Invalid(_)), "{err}");
        assert_eq!(repo.get_raw("bogus").unwrap(), None);
    }

    /// A value of the wrong JSON type is refused before anything is written, so the
    /// extraction worker never reads a shape it cannot use out of the database.
    #[test]
    fn wrong_value_types_are_rejected_before_any_write() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        let bad: &[(&str, Value)] = &[
            (API_KEY, Value::from(1)),
            (API_KEY, serde_json::json!({"k": "v"})),
            ("extraction.base_url", Value::from(true)),
            ("extraction.model", Value::from(7)),
            ("embedding.model", serde_json::json!([])),
            ("extraction.enabled", Value::String("yes".into())),
            ("extraction.auto_accept_min_confidence", Value::String("0.5".into())),
            ("extraction.auto_accept_min_confidence", Value::from(1.5)),
            ("extraction.auto_accept_min_confidence", Value::from(-0.1)),
            ("daemon.port", Value::String("7433".into())),
            ("daemon.port", Value::from(0)),
            ("daemon.port", Value::from(70000)),
        ];
        for (key, value) in bad {
            let values = Map::from_iter([((*key).to_string(), value.clone())]);
            let err = repo.set_many(&values, "t").unwrap_err();
            assert!(matches!(err, AtlasError::Invalid(_)), "{key} = {value}: {err}");
            assert_eq!(repo.get_raw(key).unwrap(), None, "{key} must not have been written");
        }
    }

    /// One bad value fails the whole call, so a partial write cannot leave the settings
    /// half updated.
    #[test]
    fn a_bad_value_rejects_the_whole_call() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        let values = Map::from_iter([
            ("extraction.model".to_string(), Value::String("deepseek-chat".into())),
            ("extraction.enabled".to_string(), Value::from(1)),
        ]);
        assert!(matches!(repo.set_many(&values, "t").unwrap_err(), AtlasError::Invalid(_)));
        assert_eq!(repo.get_raw("extraction.model").unwrap(), None);
    }

    #[test]
    fn well_typed_values_and_nulls_are_accepted() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        let values = Map::from_iter([
            ("extraction.enabled".to_string(), Value::from(true)),
            ("extraction.base_url".to_string(), Value::String("https://api.deepseek.com".into())),
            ("extraction.auto_accept_min_confidence".to_string(), Value::from(0.8)),
            ("daemon.port".to_string(), Value::from(7433)),
            ("embedding.model".to_string(), Value::Null),
        ]);
        repo.set_many(&values, "t").unwrap();
        assert_eq!(repo.get_raw("extraction.enabled").unwrap(), Some(Value::from(true)));
        assert_eq!(repo.get_raw("daemon.port").unwrap(), Some(Value::from(7433)));
        // The bounds are inclusive at both ends.
        for edge in [0.0, 1.0] {
            repo.set_many(&Map::from_iter([("extraction.auto_accept_min_confidence".to_string(), Value::from(edge))]), "t").unwrap();
        }
        // A masked api key is skipped before the type check ever sees it.
        repo.set_many(&Map::from_iter([(API_KEY.to_string(), Value::String(MASKED.into()))]), "t").unwrap();
    }

    #[test]
    fn get_all_has_every_key_and_masks_the_api_key() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        let all = repo.get_all().unwrap();
        assert_eq!(all.len(), SETTING_KEYS.len());
        for key in SETTING_KEYS {
            assert_eq!(all.get(*key), Some(&Value::Null), "{key}");
        }
        let values = Map::from_iter([(API_KEY.to_string(), Value::String("sk-real".into()))]);
        repo.set_many(&values, "t").unwrap();
        assert_eq!(repo.get_all().unwrap().get(API_KEY), Some(&Value::String(MASKED.into())));
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String("sk-real".into())));
    }

    /// A masked value sent back through `set_many` (the shape a GET->edit->PUT round
    /// trip over HTTP produces) must not overwrite the real key underneath it.
    #[test]
    fn masked_api_key_round_trip_leaves_the_real_key_unchanged() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        repo.set_many(&Map::from_iter([(API_KEY.to_string(), Value::String("sk-real".into()))]), "t").unwrap();
        repo.set_many(
            &Map::from_iter([(API_KEY.to_string(), Value::String(MASKED.into())), ("extraction.model".to_string(), Value::String("x".into()))]),
            "t",
        )
        .unwrap();
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String("sk-real".into())));
        assert_eq!(repo.get_raw("extraction.model").unwrap(), Some(Value::String("x".into())));
    }

    #[test]
    fn set_many_upserts_and_is_audited_without_the_key_value() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        repo.set_many(&Map::from_iter([("daemon.port".to_string(), Value::from(7433))]), "t").unwrap();
        repo.set_many(&Map::from_iter([("daemon.port".to_string(), Value::from(7000))]), "t").unwrap();
        assert_eq!(repo.get_raw("daemon.port").unwrap(), Some(Value::from(7000)));

        repo.set_many(&Map::from_iter([(API_KEY.to_string(), Value::String("sk-secret".into()))]), "t").unwrap();
        let detail: String = db
            .with_conn(|c| Ok(c.query_row("select detail::text from audit where entity = 'setting' and actor = 't' order by \"at\" desc limit 1", [], |r| r.get(0))?))
            .unwrap();
        assert!(!detail.contains("sk-secret"), "the api key value must not appear in the audit log: {detail}");
    }
}
