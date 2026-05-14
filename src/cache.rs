use std::{collections::HashMap, fs, path::PathBuf};

use color_eyre::eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};

use crate::{
    app::IssueFilters,
    domain::{IssueDetail, IssueSummary},
    repo::Repository,
};

#[derive(Clone, Debug)]
pub struct IssueCache {
    root: PathBuf,
    repo: Repository,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct CacheFile {
    issues: HashMap<String, Vec<IssueSummary>>,
    details: HashMap<u64, IssueDetail>,
}

impl IssueCache {
    pub fn new(root: impl Into<PathBuf>, repo: Repository) -> Self {
        Self {
            root: root.into(),
            repo,
        }
    }

    pub fn for_repo(repo: &Repository) -> Option<Self> {
        let root = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
        Some(Self::new(root.join("tissues"), repo.clone()))
    }

    pub fn repo_dir(&self) -> PathBuf {
        self.root.join(format!(
            "{}__{}",
            safe_path_segment(&self.repo.owner),
            safe_path_segment(&self.repo.name)
        ))
    }

    pub fn load_issues(&self, filters: &IssueFilters) -> Result<Option<Vec<IssueSummary>>> {
        Ok(self.load_file()?.issues.get(&filter_key(filters)?).cloned())
    }

    pub fn save_issues(&self, filters: &IssueFilters, issues: &[IssueSummary]) -> Result<()> {
        let mut file = self.load_file()?;
        file.issues.insert(filter_key(filters)?, issues.to_vec());
        self.save_file(&file)
    }

    pub fn load_detail(&self, number: u64) -> Result<Option<IssueDetail>> {
        Ok(self.load_file()?.details.get(&number).cloned())
    }

    pub fn save_detail(&self, detail: &IssueDetail) -> Result<()> {
        let mut file = self.load_file()?;
        file.details.insert(detail.summary.number, detail.clone());
        self.save_file(&file)
    }

    fn cache_file_path(&self) -> PathBuf {
        self.repo_dir().join("cache.json")
    }

    fn load_file(&self) -> Result<CacheFile> {
        let path = self.cache_file_path();
        if !path.exists() {
            return Ok(CacheFile::default());
        }

        let contents = fs::read_to_string(&path)
            .wrap_err_with(|| format!("failed to read cache file {}", path.display()))?;
        serde_json::from_str(&contents)
            .wrap_err_with(|| format!("failed to parse cache file {}", path.display()))
    }

    fn save_file(&self, file: &CacheFile) -> Result<()> {
        let dir = self.repo_dir();
        fs::create_dir_all(&dir)
            .wrap_err_with(|| format!("failed to create cache directory {}", dir.display()))?;
        let path = self.cache_file_path();
        let contents = serde_json::to_string_pretty(file).wrap_err("failed to serialize cache")?;
        fs::write(&path, contents)
            .wrap_err_with(|| format!("failed to write cache file {}", path.display()))
    }
}

fn filter_key(filters: &IssueFilters) -> Result<String> {
    serde_json::to_string(filters).wrap_err("failed to encode issue filters")
}

fn safe_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}
