use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

pub const PERMANENT_MAX: usize = 100;
pub const RECENT_MAX: usize = 10;
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize, Clone)]
pub struct PermanentItem {
    pub key: String,
    pub value: String,
    pub ts: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RecentItem {
    pub value: String,
    pub ts: u64,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Store {
    pub permanent: Vec<PermanentItem>,
    pub recent: VecDeque<RecentItem>,
}

pub struct StoreManager {
    store: Store,
    path: PathBuf,
}

impl StoreManager {
    pub fn load() -> Result<Self, String> {
        let path = dirs::home_dir()
            .ok_or("无法找到用户目录")?
            .join(".desktop-pet-clipboard/store.json");
        Self::load_from(path)
    }

    fn load_from(path: PathBuf) -> Result<Self, String> {
        let mut store: Store = match std::fs::File::open(&path) {
            Ok(file) => {
                let mut json = String::new();
                file.take(MAX_FILE_BYTES + 1)
                    .read_to_string(&mut json)
                    .map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
                if json.len() as u64 > MAX_FILE_BYTES {
                    return Err(format!("{} 超出存储大小限制", path.display()));
                }
                serde_json::from_str(&json).map_err(|e| {
                    format!("{} 数据损坏，请先备份并修复该文件：{e}", path.display())
                })?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Store::default(),
            Err(e) => return Err(format!("打开 {} 失败：{e}", path.display())),
        };

        let mut seen = HashSet::new();
        store
            .recent
            .retain(|item| !item.value.trim().is_empty() && seen.insert(item.value.clone()));
        store.recent.truncate(RECENT_MAX);
        Ok(Self { store, path })
    }

    fn commit(&mut self, next: Store) -> Result<(), String> {
        let parent = self.path.parent().ok_or("无效的存储路径")?;
        let mut dir = std::fs::DirBuilder::new();
        dir.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            dir.mode(0o700);
        }
        dir.create(parent)
            .map_err(|e| format!("创建存储目录失败：{e}"))?;

        let temp = parent.join(format!(
            ".store-{}-{}.tmp",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp)
            .map_err(|e| format!("创建临时存储文件失败：{e}"))?;
        let saved = (|| -> Result<(), String> {
            let json = serde_json::to_vec(&next).map_err(|e| e.to_string())?;
            if json.len() as u64 > MAX_FILE_BYTES {
                return Err("数据超出存储大小限制".into());
            }
            file.write_all(&json).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            drop(file);
            // Rename only after a complete write; failed saves never change in-memory data.
            std::fs::rename(&temp, &self.path).map_err(|e| e.to_string())
        })();
        if let Err(error) = saved {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("保存剪贴板失败：{error}"));
        }
        self.store = next;
        Ok(())
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// 添加/覆盖永久条目；满 100 条且是新 key 时返回 Err
    pub fn add_permanent(&mut self, key: &str, value: &str) -> Result<(), String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("key 不能为空".into());
        }
        if key.len() > 1024 {
            return Err("key 不能超过 1 KB".into());
        }
        if value.len() > MAX_TEXT_BYTES {
            return Err("内容不能超过 1 MB".into());
        }
        if let Some(item) = self.store.permanent.iter().find(|i| i.key == key) {
            if item.value == value {
                return Ok(());
            }
        } else if self.store.permanent.len() >= PERMANENT_MAX {
            return Err(format!("永久存已满 {} 条，请先删除一些", PERMANENT_MAX));
        }

        let mut next = self.store.clone();
        if let Some(item) = next.permanent.iter_mut().find(|i| i.key == key) {
            item.value = value.to_string();
            item.ts = Self::now();
        } else {
            next.permanent.push(PermanentItem {
                key: key.to_string(),
                value: value.to_string(),
                ts: Self::now(),
            });
        }
        self.commit(next)
    }

    pub fn remove_permanent(&mut self, key: &str) -> Result<(), String> {
        if !self.store.permanent.iter().any(|item| item.key == key) {
            return Ok(());
        }
        let mut next = self.store.clone();
        next.permanent.retain(|i| i.key != key);
        self.commit(next)
    }

    /// 模糊搜索；query 为空返回全部（按时间倒序）
    pub fn search_permanent(&self, query: &str) -> Vec<&PermanentItem> {
        let q = query.trim();
        if q.is_empty() {
            let mut all: Vec<&PermanentItem> = self.store.permanent.iter().collect();
            all.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| a.key.cmp(&b.key)));
            return all;
        }
        use fuzzy_matcher::skim::SkimMatcherV2;
        use fuzzy_matcher::FuzzyMatcher;
        let matcher = SkimMatcherV2::default();
        let mut scored: Vec<(i64, &PermanentItem)> = self
            .store
            .permanent
            .iter()
            .filter_map(|item| matcher.fuzzy_match(&item.key, q).map(|s| (s, item)))
            .collect();
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| b.1.ts.cmp(&a.1.ts))
                .then_with(|| a.1.key.cmp(&b.1.key))
        });
        scored.into_iter().map(|(_, item)| item).collect()
    }

    /// 新复制内容进最近队列：去重置顶，超 10 淘汰最旧
    pub fn push_recent(&mut self, value: &str) -> Result<bool, String> {
        if value.trim().is_empty()
            || value.len() > MAX_TEXT_BYTES
            || self.store.recent.front().is_some_and(|i| i.value == value)
        {
            return Ok(false);
        }
        let mut next = self.store.clone();
        next.recent.retain(|i| i.value != value);
        next.recent.push_front(RecentItem {
            value: value.to_string(),
            ts: Self::now(),
        });
        next.recent.truncate(RECENT_MAX);
        self.commit(next)?;
        Ok(true)
    }

    pub fn recent_list(&self) -> impl Iterator<Item = &RecentItem> {
        self.store.recent.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "desktop-pet-store-test-{}-{}",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn manager(&self) -> StoreManager {
            StoreManager::load_from(self.0.join("store.json")).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn round_trip_preserves_unicode_and_whitespace() {
        let fixture = Fixture::new();
        let mut store = fixture.manager();
        store.add_permanent("  邮箱  ", "  user@example.com\n").unwrap();
        store.push_recent("复制内容\n").unwrap();
        let loaded = fixture.manager();
        assert_eq!(loaded.search_permanent("邮")[0].key, "邮箱");
        assert_eq!(loaded.search_permanent("")[0].value, "  user@example.com\n");
        assert_eq!(loaded.recent_list().next().unwrap().value, "复制内容\n");
    }

    #[test]
    fn validates_key_and_value_sizes() {
        let fixture = Fixture::new();
        let mut store = fixture.manager();
        assert!(store.add_permanent(" \n", "value").is_err());
        assert!(store.add_permanent(&"k".repeat(1025), "value").is_err());
        assert!(store.add_permanent("key", &"x".repeat(MAX_TEXT_BYTES + 1)).is_err());
        assert!(!store.push_recent(" \n ").unwrap());
        assert!(!store.push_recent(&"x".repeat(MAX_TEXT_BYTES + 1)).unwrap());
        assert!(!store.path.exists());
    }

    #[test]
    fn permanent_limit_allows_updates_and_deletions() {
        let fixture = Fixture::new();
        let mut store = fixture.manager();
        for n in 0..PERMANENT_MAX {
            store.add_permanent(&format!("key{n}"), "value").unwrap();
        }
        assert!(store.add_permanent("overflow", "value").is_err());
        store.add_permanent("key0", "updated").unwrap();
        assert_eq!(store.store.permanent.len(), PERMANENT_MAX);
        store.remove_permanent("key1").unwrap();
        store.add_permanent("replacement", "value").unwrap();
        assert_eq!(fixture.manager().store.permanent.len(), PERMANENT_MAX);
    }

    #[test]
    fn recent_deduplicates_promotes_and_evicts() {
        let fixture = Fixture::new();
        let mut store = fixture.manager();
        for n in 0..=RECENT_MAX {
            assert!(store.push_recent(&n.to_string()).unwrap());
        }
        assert_eq!(store.store.recent.len(), RECENT_MAX);
        assert!(!store.store.recent.iter().any(|i| i.value == "0"));
        assert!(store.push_recent("5").unwrap());
        assert_eq!(store.store.recent.front().unwrap().value, "5");
        assert_eq!(store.store.recent.iter().filter(|i| i.value == "5").count(), 1);
        assert!(!store.push_recent("5").unwrap());
    }

    #[test]
    fn unchanged_operations_do_not_write() {
        let fixture = Fixture::new();
        let mut store = fixture.manager();
        store.add_permanent("key", "value").unwrap();
        store.push_recent("recent").unwrap();
        std::fs::remove_file(&store.path).unwrap();
        store.add_permanent("key", "value").unwrap();
        store.remove_permanent("missing").unwrap();
        assert!(!store.push_recent("recent").unwrap());
        assert!(!store.path.exists());
    }

    #[test]
    fn failed_save_rolls_back_memory_and_removes_temp_file() {
        let fixture = Fixture::new();
        let mut store = fixture.manager();
        store.add_permanent("key", "original").unwrap();
        store.push_recent("original").unwrap();
        let blocked = fixture.0.join("blocked");
        std::fs::create_dir(&blocked).unwrap();
        store.path = blocked;
        assert!(store.add_permanent("key", "changed").is_err());
        assert_eq!(store.search_permanent("")[0].value, "original");
        assert!(store.remove_permanent("key").is_err());
        assert_eq!(store.search_permanent("").len(), 1);
        assert!(store.push_recent("changed").is_err());
        assert_eq!(store.recent_list().next().unwrap().value, "original");
        assert!(!std::fs::read_dir(&fixture.0).unwrap().any(|entry| {
            entry.unwrap().file_name().to_string_lossy().ends_with(".tmp")
        }));
        assert_eq!(fixture.manager().search_permanent("")[0].value, "original");
    }

    #[test]
    fn corrupt_data_is_not_silently_replaced() {
        let fixture = Fixture::new();
        let path = fixture.0.join("store.json");
        std::fs::write(&path, "{broken json").unwrap();
        assert!(StoreManager::load_from(path.clone()).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "{broken json");
    }

    #[test]
    fn older_schema_can_omit_recent() {
        let fixture = Fixture::new();
        std::fs::write(fixture.0.join("store.json"), r#"{"permanent":[]}"#).unwrap();
        assert_eq!(fixture.manager().recent_list().count(), 0);
    }

    #[test]
    fn load_normalizes_oversized_and_duplicate_history() {
        let fixture = Fixture::new();
        let mut recent: Vec<RecentItem> = (0..20)
            .map(|n| RecentItem { value: n.to_string(), ts: n })
            .collect();
        recent.insert(0, RecentItem { value: "0".into(), ts: 30 });
        recent.insert(0, RecentItem { value: " ".into(), ts: 31 });
        std::fs::write(
            fixture.0.join("store.json"),
            serde_json::to_vec(&Store { permanent: vec![], recent: recent.into() }).unwrap(),
        ).unwrap();
        let store = fixture.manager();
        assert_eq!(store.recent_list().count(), RECENT_MAX);
        assert_eq!(store.recent_list().next().unwrap().value, "0");
        assert_eq!(store.store.recent.iter().filter(|i| i.value == "0").count(), 1);
    }

    #[test]
    fn search_supports_fuzzy_matching_and_stable_ties() {
        let fixture = Fixture::new();
        let mut store = fixture.manager();
        store.add_permanent("beta", "b").unwrap();
        store.add_permanent("alpha", "a").unwrap();
        for item in &mut store.store.permanent { item.ts = 1; }
        assert_eq!(store.search_permanent("  ")[0].key, "alpha");
        assert_eq!(store.search_permanent("ap")[0].key, "alpha");
        assert!(store.search_permanent("unmatched").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn stored_clipboard_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::new();
        let mut store = fixture.manager();
        store.add_permanent("secret", "private").unwrap();
        assert_eq!(std::fs::metadata(&store.path).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
