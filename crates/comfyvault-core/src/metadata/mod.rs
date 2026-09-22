//! Looking a model up on Civitai by its hash, and remembering the answer.
//!
//! # This never breaks anything
//!
//! A metadata lookup is optional in every sense. A file with no match is the
//! normal outcome for anything the person trained or downloaded elsewhere, and
//! it is not an error. A network that is unreachable is not an error either:
//! the engine answers from what it already knows and says nothing was found.
//! No scan, plan or apply ever waits on a lookup or fails because of one.

pub mod civitai;
pub mod http;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::store::Store;
use crate::time_util::Timestamp;

/// What is known about one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMetadata {
    pub sha256: String,
    pub source: MetadataSourceName,
    pub fetched_at: Timestamp,
    /// `false` is a normal answer, not a failure.
    pub found: bool,
    pub model_name: Option<String>,
    pub model_type: Option<String>,
    pub version_name: Option<String>,
    /// Free text that grows as new model families appear. Never a fixed list.
    pub base_model: Option<String>,
    pub trigger_words: Vec<String>,
    pub nsfw: bool,
    pub nsfw_level: u32,
    pub civitai_model_id: Option<u64>,
    pub civitai_version_id: Option<u64>,
    pub page_url: Option<String>,
    pub download_url: Option<String>,
    pub preview_image_urls: Vec<String>,
    /// The hash matched more than one published model version, because people
    /// re-upload identical files under their own pages.
    pub ambiguous: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MetadataSourceName {
    Civitai,
}

impl ModelMetadata {
    /// A record that says "nothing matches this file", which is cached like any
    /// other answer so the same question is not asked twice.
    pub fn not_found(sha256: &str) -> Self {
        Self {
            sha256: sha256.to_string(),
            source: MetadataSourceName::Civitai,
            fetched_at: Timestamp::now(),
            found: false,
            model_name: None,
            model_type: None,
            version_name: None,
            base_model: None,
            trigger_words: Vec::new(),
            nsfw: false,
            nsfw_level: 0,
            civitai_model_id: None,
            civitai_version_id: None,
            page_url: None,
            download_url: None,
            preview_image_urls: Vec::new(),
            ambiguous: false,
        }
    }
}

/// Somewhere metadata can be fetched from.
pub trait MetadataSource: Send + Sync {
    /// Looks one hash up.
    fn fetch_one(&self, sha256: &str) -> Result<ModelMetadata>;
    /// Looks several hashes up, in as few requests as the service allows.
    fn fetch_many(&self, sha256: &[String]) -> Result<Vec<ModelMetadata>>;
}

/// The cache in front of a source.
pub struct MetadataService<'a> {
    store: &'a Store,
    source: &'a dyn MetadataSource,
    enabled: bool,
}

impl<'a> MetadataService<'a> {
    pub fn new(store: &'a Store, source: &'a dyn MetadataSource, enabled: bool) -> Self {
        Self { store, source, enabled }
    }

    /// Answers for one file.
    ///
    /// Reads the cache first. When lookups are switched off, or the network
    /// refuses, it answers from the cache alone and returns `None` for anything
    /// it has never seen. It does not fail.
    pub fn get(&self, sha256: &str, refresh: bool) -> Result<Option<ModelMetadata>> {
        let Some(sha) = crate::scan::hash::normalize_sha256(sha256) else {
            return Err(crate::VaultError::invalid("That is not a file hash."));
        };
        if !refresh {
            if let Some(cached) = self.store.metadata(&sha)? {
                return Ok(Some(cached));
            }
        }
        if !self.enabled {
            return Ok(self.store.metadata(&sha)?);
        }
        match self.source.fetch_one(&sha) {
            Ok(m) => {
                self.store.put_metadata(&m)?;
                Ok(Some(m))
            }
            // Offline is not a failure the caller has to handle. It means the
            // engine knows nothing more than it already did.
            Err(e) if e.code == crate::ErrorCode::NetworkUnavailable => {
                Ok(self.store.metadata(&sha)?)
            }
            Err(e) => Err(e),
        }
    }

    /// Answers for many files, asking the network only about the ones the cache
    /// does not already hold.
    pub fn get_many(&self, hashes: &[String], refresh: bool) -> Result<Vec<ModelMetadata>> {
        let mut out: Vec<ModelMetadata> = Vec::new();
        let mut to_fetch: Vec<String> = Vec::new();

        for h in hashes {
            let Some(sha) = crate::scan::hash::normalize_sha256(h) else { continue };
            match (refresh, self.store.metadata(&sha)?) {
                (false, Some(cached)) => out.push(cached),
                _ => to_fetch.push(sha),
            }
        }

        if to_fetch.is_empty() || !self.enabled {
            // Anything with no cached answer is reported as not found, so the
            // caller always gets one record per hash it asked about.
            out.extend(to_fetch.iter().map(|s| ModelMetadata::not_found(s)));
            return Ok(out);
        }

        match self.source.fetch_many(&to_fetch) {
            Ok(fetched) => {
                for m in &fetched {
                    self.store.put_metadata(m)?;
                }
                out.extend(fetched);
            }
            Err(e) if e.code == crate::ErrorCode::NetworkUnavailable && !refresh => {
                out.extend(to_fetch.iter().map(|s| ModelMetadata::not_found(s)));
            }
            Err(e) => return Err(e),
        }
        Ok(out)
    }

    pub fn clear_cache(&self) -> Result<usize> {
        self.store.clear_metadata()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A source that counts how often it was asked, so the tests can prove the
    /// cache actually prevents a request.
    struct CountingSource {
        known: Vec<String>,
        calls: Mutex<Vec<Vec<String>>>,
        offline: bool,
    }

    impl CountingSource {
        fn new(known: &[&str]) -> Self {
            Self {
                known: known.iter().map(|s| s.to_string()).collect(),
                calls: Mutex::new(Vec::new()),
                offline: false,
            }
        }
        fn offline() -> Self {
            Self { known: Vec::new(), calls: Mutex::new(Vec::new()), offline: true }
        }
        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
        fn hit(&self, sha: &str) -> ModelMetadata {
            ModelMetadata {
                found: true,
                model_name: Some(format!("Model for {}", &sha[..4])),
                ..ModelMetadata::not_found(sha)
            }
        }
    }

    impl MetadataSource for CountingSource {
        fn fetch_one(&self, sha256: &str) -> Result<ModelMetadata> {
            self.calls.lock().unwrap().push(vec![sha256.to_string()]);
            if self.offline {
                return Err(crate::VaultError::new(
                    crate::ErrorCode::NetworkUnavailable,
                    "offline",
                ));
            }
            Ok(if self.known.iter().any(|k| k == sha256) {
                self.hit(sha256)
            } else {
                ModelMetadata::not_found(sha256)
            })
        }

        fn fetch_many(&self, sha256: &[String]) -> Result<Vec<ModelMetadata>> {
            self.calls.lock().unwrap().push(sha256.to_vec());
            if self.offline {
                return Err(crate::VaultError::new(
                    crate::ErrorCode::NetworkUnavailable,
                    "offline",
                ));
            }
            Ok(sha256
                .iter()
                .map(|s| if self.known.contains(s) { self.hit(s) } else { ModelMetadata::not_found(s) })
                .collect())
        }
    }

    fn sha(n: u8) -> String {
        format!("{:064X}", n)
    }

    fn store() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("vault"), true).unwrap();
        (d, s)
    }

    #[test]
    fn a_hit_is_returned_and_remembered() {
        let (_d, store) = store();
        let src = CountingSource::new(&[sha(1).as_str()]);
        let svc = MetadataService::new(&store, &src, true);

        let first = svc.get(&sha(1), false).unwrap().unwrap();
        assert!(first.found);
        assert_eq!(src.call_count(), 1);

        let second = svc.get(&sha(1), false).unwrap().unwrap();
        assert!(second.found);
        assert_eq!(src.call_count(), 1, "the cache did not prevent a second request");
    }

    #[test]
    fn a_miss_is_a_normal_answer_and_is_also_remembered() {
        // Without caching the miss, every screen refresh would ask the network
        // again about every file the person trained themselves.
        let (_d, store) = store();
        let src = CountingSource::new(&[]);
        let svc = MetadataService::new(&store, &src, true);

        let m = svc.get(&sha(9), false).unwrap().unwrap();
        assert!(!m.found);
        assert!(m.model_name.is_none());

        let _ = svc.get(&sha(9), false).unwrap();
        assert_eq!(src.call_count(), 1, "a miss must be cached like a hit");
    }

    #[test]
    fn refresh_asks_again_even_when_the_answer_is_cached() {
        let (_d, store) = store();
        let src = CountingSource::new(&[sha(1).as_str()]);
        let svc = MetadataService::new(&store, &src, true);

        svc.get(&sha(1), false).unwrap();
        svc.get(&sha(1), true).unwrap();
        assert_eq!(src.call_count(), 2);
    }

    #[test]
    fn switching_lookups_off_answers_from_the_cache_alone() {
        let (_d, store) = store();
        let src = CountingSource::new(&[sha(1).as_str()]);

        MetadataService::new(&store, &src, true).get(&sha(1), false).unwrap();
        assert_eq!(src.call_count(), 1);

        let off = MetadataService::new(&store, &src, false);
        assert!(off.get(&sha(1), false).unwrap().unwrap().found, "the cache still answers");
        assert!(off.get(&sha(2), false).unwrap().is_none(), "an unknown file answers nothing");
        assert_eq!(src.call_count(), 1, "nothing may reach the network when lookups are off");
    }

    #[test]
    fn a_network_failure_never_reaches_the_caller_as_an_error() {
        // The whole point: the app works fully offline.
        let (_d, store) = store();
        let src = CountingSource::offline();
        let svc = MetadataService::new(&store, &src, true);

        let got = svc.get(&sha(3), false).unwrap();
        assert!(got.is_none(), "unknown and offline answers nothing, and does not fail");
    }

    #[test]
    fn a_network_failure_still_serves_what_was_cached_earlier() {
        let (_d, store) = store();
        let online = CountingSource::new(&[sha(1).as_str()]);
        MetadataService::new(&store, &online, true).get(&sha(1), false).unwrap();

        let offline = CountingSource::offline();
        let svc = MetadataService::new(&store, &offline, true);
        assert!(svc.get(&sha(1), false).unwrap().unwrap().found);
    }

    #[test]
    fn a_batch_asks_only_about_what_is_not_cached() {
        let (_d, store) = store();
        let src = CountingSource::new(&[sha(1).as_str(), sha(2).as_str()]);
        let svc = MetadataService::new(&store, &src, true);

        svc.get(&sha(1), false).unwrap();
        src.calls.lock().unwrap().clear();

        let got = svc.get_many(&[sha(1), sha(2), sha(3)], false).unwrap();
        assert_eq!(got.len(), 3, "one record per hash asked about");

        let calls = src.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1, "one batch request");
        assert_eq!(calls[0], vec![sha(2), sha(3)], "the cached hash must not be asked about again");
    }

    #[test]
    fn a_batch_returns_one_record_per_hash_even_when_offline() {
        let (_d, store) = store();
        let src = CountingSource::offline();
        let svc = MetadataService::new(&store, &src, true);

        let got = svc.get_many(&[sha(1), sha(2)], false).unwrap();
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|m| !m.found));
    }

    #[test]
    fn text_that_is_not_a_hash_is_refused_rather_than_sent_to_the_network() {
        let (_d, store) = store();
        let src = CountingSource::new(&[]);
        let svc = MetadataService::new(&store, &src, true);

        let err = svc.get("not-a-hash", false).unwrap_err();
        assert_eq!(err.code, crate::ErrorCode::InvalidArgument);
        assert_eq!(src.call_count(), 0);
    }

    #[test]
    fn a_lower_case_hash_finds_the_same_cached_record() {
        let (_d, store) = store();
        let src = CountingSource::new(&[sha(1).as_str()]);
        let svc = MetadataService::new(&store, &src, true);

        svc.get(&sha(1), false).unwrap();
        svc.get(&sha(1).to_lowercase(), false).unwrap();
        assert_eq!(src.call_count(), 1, "case must not split the cache");
    }

    #[test]
    fn clearing_the_cache_makes_the_next_question_reach_the_source() {
        let (_d, store) = store();
        let src = CountingSource::new(&[sha(1).as_str()]);
        let svc = MetadataService::new(&store, &src, true);

        svc.get(&sha(1), false).unwrap();
        assert_eq!(svc.clear_cache().unwrap(), 1);
        svc.get(&sha(1), false).unwrap();
        assert_eq!(src.call_count(), 2);
    }
}
