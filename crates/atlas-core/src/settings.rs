use crate::db::Db;
use crate::memories::MemoryRepo;
use crate::{AtlasError, Result};
use duckdb::params;
use serde_json::{Map, Value};

/// The only keys the settings table accepts. `set_many` rejects anything else.
/// `board.stages` is readable here but not writable: see [`SettingsRepo::set_many`].
pub const SETTING_KEYS: &[&str] = &[
    "extraction.enabled",
    "extraction.base_url",
    "extraction.api_key",
    "extraction.model",
    "extraction.auto_accept_min_confidence",
    "embedding.model",
    "daemon.port",
    "board.stages",
    "board.mirror_tasks_md",
    "ui.theme",
    "workflows.docs_migrated",
];

const API_KEY: &str = "extraction.api_key";
const BASE_URL: &str = "extraction.base_url";
const STAGES: &str = "board.stages";
pub(crate) const MASKED: &str = "***";

/// Whether two base urls name the same endpoint. A trailing slash is not a change
/// of endpoint, and `LlmClient` strips one anyway before building its request url.
pub(crate) fn same_endpoint(a: &str, b: &str) -> bool {
    a.trim().trim_end_matches('/') == b.trim().trim_end_matches('/')
}

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
        // The board refuses a stage list the repository could not use, before it is
        // stored, so a bad list can never reach a task move.
        "board.stages" => crate::board::parse_stages(value.clone()).map(|_| ())?,
        // A latch, not a preference: `workflow::migrate_docs` sets it once the Markdown
        // workflow documents have become workflows, and reads it to know not to run again.
        "board.mirror_tasks_md" | "workflows.docs_migrated" => {
            if !value.is_boolean() {
                return wrong("a boolean");
            }
        }
        // The desktop mirrors its theme here so a second client opens on the same ramp.
        "ui.theme" => match value.as_str() {
            Some("dark") | Some("light") => {}
            _ => return wrong("\"dark\" or \"light\""),
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
    /// unknown, is `board.stages`, or holds a value of the wrong type, before writing
    /// anything. Ignores
    /// `extraction.api_key == "***"` so a masked value read back from `get_all` and
    /// sent straight back does not clobber the real key.
    ///
    /// Answers whether the stored api key was cleared. The daemon has no
    /// authentication of its own, so any process that can reach the loopback port can
    /// repoint `extraction.base_url` and then have the daemon send the stored key to
    /// an endpoint of its choosing. A key entered against one endpoint is therefore
    /// not a key for another: changing the base url without supplying a new key
    /// clears the stored one, and the next model call fails until it is entered again.
    pub fn set_many(&self, values: &Map<String, Value>, actor: &str) -> Result<bool> {
        for key in values.keys() {
            if !SETTING_KEYS.contains(&key.as_str()) {
                return Err(AtlasError::Invalid(format!("unknown setting key '{key}'")));
            }
        }
        // The board route is the only way in. It checks that no stage being dropped
        // still holds tasks, applies the `renames` map that carries tasks across, and
        // takes the task write gate; this call can do none of that. A list written
        // behind the board's back strands tasks in a stage no surface can show or
        // count, and clearing the list back to the default is `PUT /board/stages` with
        // the default four.
        if values.contains_key(STAGES) {
            return Err(AtlasError::Invalid("set board stages through PUT /api/v1/board/stages".into()));
        }
        for (key, value) in values {
            check_type(key, value)?;
        }
        let clear_key = self.base_url_moves_away_from_the_stored_key(values)?;
        for (key, value) in values {
            if key == API_KEY && value.as_str() == Some(MASKED) {
                continue;
            }
            self.write(key, value)?;
            // The api key value itself never goes into the audit log.
            let detail = if key == API_KEY { serde_json::json!({"key": key}) } else { serde_json::json!({"key": key, "value": value}) };
            MemoryRepo::new(self.db).audit(actor, "set", "setting", None, detail)?;
        }
        if clear_key {
            self.write(API_KEY, &Value::String(String::new()))?;
            MemoryRepo::new(self.db).audit(actor, "clear", "setting", None, serde_json::json!({"key": API_KEY, "reason": "base_url changed"}))?;
            // The key itself is never named in the log, only the fact that it is gone.
            tracing::info!("extraction.base_url changed without a new api key; the stored key was cleared");
        }
        Ok(clear_key)
    }

    /// Stores the global stage list. Crate-private because
    /// [`crate::board::TaskRepo::set_global_stages`] is the only caller that has done
    /// the removal checks, the renames and the gate hold that `set_many` refuses to
    /// write without.
    pub(crate) fn set_board_stages(&self, stages: &Value, actor: &str) -> Result<()> {
        check_type(STAGES, stages)?;
        self.write(STAGES, stages)?;
        MemoryRepo::new(self.db).audit(actor, "set", "setting", None, serde_json::json!({"key": STAGES, "value": stages}))
    }

    /// Whether this call points `extraction.base_url` somewhere new while leaving the
    /// stored api key in place. A masked `"***"` is the GUI saying "leave the key
    /// alone", not a key, so it does not count as supplying one.
    fn base_url_moves_away_from_the_stored_key(&self, values: &Map<String, Value>) -> Result<bool> {
        let Some(incoming) = values.get(BASE_URL).and_then(|v| v.as_str()) else { return Ok(false) };
        let sets_key = values.get(API_KEY).and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty() && s != MASKED);
        if sets_key {
            return Ok(false);
        }
        let stored_key = self.get_raw(API_KEY)?.and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        if stored_key.is_empty() {
            return Ok(false);
        }
        let stored_url = self.get_raw(BASE_URL)?.and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        Ok(!same_endpoint(incoming, &stored_url))
    }

    fn write(&self, key: &str, value: &Value) -> Result<()> {
        let json = value.to_string();
        self.db.with_conn(|c| {
            c.execute("delete from settings where key = ?", params![key])?;
            c.execute("insert into settings (key, value) values (?, ?::json)", params![key, json])?;
            Ok(())
        })
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
            ("ui.theme", Value::String("solarized".into())),
            ("ui.theme", Value::from(1)),
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
            ("ui.theme".to_string(), Value::String("light".into())),
        ]);
        repo.set_many(&values, "t").unwrap();
        assert_eq!(repo.get_raw("ui.theme").unwrap(), Some(Value::String("light".into())));
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

    /// The daemon is unauthenticated on loopback, so a local process can repoint
    /// `extraction.base_url` and have the daemon send the stored key wherever it likes.
    /// A key belongs to the endpoint it was entered against: moving the endpoint drops
    /// it, and the caller is told so.
    #[test]
    fn changing_the_base_url_clears_the_stored_api_key() {
        let db = Db::open_in_memory().unwrap();
        let repo = SettingsRepo::new(&db);
        let set = |values: Vec<(&str, Value)>| {
            repo.set_many(&Map::from_iter(values.into_iter().map(|(k, v)| (k.to_string(), v))), "t").unwrap()
        };

        // Configured in one call: the key is for the endpoint named beside it.
        assert!(!set(vec![(BASE_URL, "https://api.deepseek.com".into()), (API_KEY, "sk-real".into())]));
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String("sk-real".into())));

        // Re-sending the same endpoint, with or without its trailing slash, is not a move.
        assert!(!set(vec![(BASE_URL, "https://api.deepseek.com/".into())]));
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String("sk-real".into())));
        // Nor is a masked key alongside it: `"***"` means "leave the key alone".
        assert!(!set(vec![(BASE_URL, "https://api.deepseek.com".into()), (API_KEY, MASKED.into())]));
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String("sk-real".into())));

        // A new endpoint with no new key: the old key does not follow it there.
        assert!(set(vec![(BASE_URL, "http://attacker.example/v1".into())]));
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String(String::new())));
        assert_eq!(repo.get_raw(BASE_URL).unwrap(), Some(Value::String("http://attacker.example/v1".into())), "the endpoint itself is still stored");

        // A new endpoint that brings its own key keeps it.
        assert!(!set(vec![(BASE_URL, "http://localhost:1234/v1".into()), (API_KEY, "sk-local".into())]));
        assert_eq!(repo.get_raw(API_KEY).unwrap(), Some(Value::String("sk-local".into())));

        // With no key stored there is nothing to clear, so nothing is reported.
        repo.set_many(&Map::from_iter([(API_KEY.to_string(), Value::String(String::new()))]), "t").unwrap();
        assert!(!set(vec![(BASE_URL, "http://elsewhere.example/v1".into())]));
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
