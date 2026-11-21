//! Persistent, owner-private JSON cache of command enrichment, keyed by an
//! executable fingerprint so upgrades and replacements invalidate entries.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::atomic::Ordering;

use super::super::{
    CACHE_MAX_BYTES, CACHE_VERSION, CachedDescription, DescriptionCache, DescriptionJob,
    ExecutableFingerprint, MAX_OPTION_COUNT, MAX_SUBCOMMAND_COUNT, MAX_VALUE_COUNT, MISS_TTL,
    SUCCESS_TTL, TEMP_SEQUENCE, valid_subcommand_name, valid_value,
};
use super::parse::{sanitize_description, valid_option_spelling};

pub(crate) fn load_cache(path: &Path) -> DescriptionCache {
    let Ok(metadata) = fs::metadata(path) else {
        return DescriptionCache::default();
    };
    if metadata.len() > CACHE_MAX_BYTES {
        return DescriptionCache::default();
    }
    let Ok(bytes) = fs::read(path) else {
        return DescriptionCache::default();
    };
    let Ok(mut cache) = serde_json::from_slice::<DescriptionCache>(&bytes) else {
        return DescriptionCache::default();
    };
    if cache.version != CACHE_VERSION {
        return DescriptionCache::default();
    }
    for cached in cache.entries.values_mut() {
        let mut seen = HashSet::new();
        cached.options.retain_mut(|option| {
            if !valid_option_spelling(&option.spelling) || !seen.insert(option.spelling.clone()) {
                return false;
            }
            option.description = sanitize_description(&option.description).unwrap_or_default();
            true
        });
        cached.options.truncate(MAX_OPTION_COUNT);

        seen.clear();
        cached.subcommands.retain_mut(|subcommand| {
            if !valid_subcommand_name(&subcommand.name) || !seen.insert(subcommand.name.clone()) {
                return false;
            }
            subcommand.description =
                sanitize_description(&subcommand.description).unwrap_or_default();
            true
        });
        cached.subcommands.truncate(MAX_SUBCOMMAND_COUNT);

        seen.clear();
        cached.option_values.retain_mut(|option_values| {
            if !valid_option_spelling(&option_values.option)
                || !seen.insert(option_values.option.clone())
            {
                return false;
            }
            let mut seen_values = HashSet::new();
            option_values.values.retain_mut(|value| {
                if !valid_value(&value.value) || !seen_values.insert(value.value.clone()) {
                    return false;
                }
                value.description = sanitize_description(&value.description).unwrap_or_default();
                true
            });
            option_values.values.truncate(MAX_VALUE_COUNT);
            !option_values.values.is_empty()
        });
    }
    cache
}

pub(crate) fn save_cache(path: &Path, cache: &DescriptionCache) -> std::io::Result<()> {
    let mut bounded = cache.clone();
    let bytes = loop {
        let bytes = serde_json::to_vec(&bounded)?;
        if bytes.len() as u64 <= CACHE_MAX_BYTES || bounded.entries.is_empty() {
            break bytes;
        }
        let Some(oldest) = bounded
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.checked_at_secs)
            .map(|(name, _)| name.clone())
        else {
            break bytes;
        };
        bounded.entries.remove(&oldest);
    };
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("tmp-{}-{sequence}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub(crate) fn cache_is_fresh(cached: &CachedDescription, now: u64) -> bool {
    let ttl = if !cached.options.is_empty()
        || !cached.subcommands.is_empty()
        || !cached.option_values.is_empty()
    {
        SUCCESS_TTL
    } else {
        MISS_TTL
    };
    now >= cached.checked_at_secs && now - cached.checked_at_secs <= ttl.as_secs()
}

pub(crate) fn fingerprint(path: &Path, metadata: &fs::Metadata) -> ExecutableFingerprint {
    ExecutableFingerprint {
        path: path.to_path_buf(),
        size: metadata.len(),
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        modified_secs: metadata.mtime(),
        modified_nanos: metadata.mtime_nsec(),
        changed_secs: metadata.ctime(),
        changed_nanos: metadata.ctime_nsec(),
    }
}

pub(crate) fn fingerprint_matches(job: &DescriptionJob) -> bool {
    fs::metadata(&job.path)
        .map(|metadata| fingerprint(&job.path, &metadata) == job.fingerprint)
        .unwrap_or(false)
}
