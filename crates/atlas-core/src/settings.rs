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
    /// unknown, before writing anything. Ignores `extraction.api_key == "***"` so a
    /// masked value read back from `get_all` and sent straight back does not clobber
    /// the real key.
    pub fn set_many(&self, values: &Map<String, Value>, actor: &str) -> Result<()> {
        for key in values.keys() {
            if !SETTING_KEYS.contains(&key.as_str()) {
                return Err(AtlasError::Invalid(format!("unknown setting key '{key}'")));
            }
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
