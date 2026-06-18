use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::{AssigneeFilter, IssueFilters, IssueSort, IssueStateFilter};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SavedView {
    pub name: String,
    pub filters: IssueFilters,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub views: Vec<SavedView>,
    #[serde(default)]
    pub auth: GitHubAuthConfig,
    #[serde(default)]
    pub project_board: ProjectBoardConfig,
    #[serde(default)]
    pub ui: UiEffectsConfig,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadingEffectStyle {
    Disabled,
    #[default]
    Coalesce,
    Paint,
    Evolve,
    Explode,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct UiEffectsConfig {
    #[serde(default)]
    pub all_effects_disabled: bool,
    #[serde(default)]
    pub loading_effect: LoadingEffectStyle,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct GitHubAuthConfig {
    pub gh_user: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectBoardConfig {
    pub owner: Option<String>,
    pub number: Option<u32>,
    #[serde(default = "default_project_status_field")]
    pub status_field: String,
}

impl Default for ProjectBoardConfig {
    fn default() -> Self {
        Self {
            owner: None,
            number: None,
            status_field: default_project_status_field(),
        }
    }
}

impl ProjectBoardConfig {
    pub fn is_configured(&self) -> bool {
        self.number.is_some()
    }
}

fn default_project_status_field() -> String {
    "Status".to_string()
}

impl AppConfig {
    pub fn with_builtin_views() -> Self {
        Self {
            views: vec![
                SavedView {
                    name: "mine".to_string(),
                    filters: IssueFilters {
                        state: IssueStateFilter::Open,
                        assignee: AssigneeFilter::Me,
                        labels: Vec::new(),
                        query: String::new(),
                        sort: IssueSort::Updated,
                    },
                },
                SavedView {
                    name: "untriaged".to_string(),
                    filters: IssueFilters {
                        state: IssueStateFilter::Open,
                        assignee: AssigneeFilter::None,
                        labels: Vec::new(),
                        query: String::new(),
                        sort: IssueSort::Updated,
                    },
                },
                SavedView {
                    name: "bugs".to_string(),
                    filters: IssueFilters {
                        state: IssueStateFilter::Open,
                        assignee: AssigneeFilter::Any,
                        labels: vec!["bug".to_string()],
                        query: String::new(),
                        sort: IssueSort::Updated,
                    },
                },
            ],
            auth: GitHubAuthConfig::default(),
            project_board: ProjectBoardConfig::default(),
            ui: UiEffectsConfig::default(),
        }
    }

    pub fn load() -> Self {
        Self::load_from_paths(config_path(), repo_config_path())
    }

    pub fn load_from_paths(user_path: Option<PathBuf>, repo_path: Option<PathBuf>) -> Self {
        let mut config = Self::with_builtin_views();
        config.merge_path(user_path);
        config.merge_path(repo_path);
        config
    }

    fn merge_path(&mut self, path: Option<PathBuf>) {
        let Some(path) = path else {
            return;
        };
        let Ok(contents) = fs::read_to_string(path) else {
            return;
        };
        let Ok(user_config) = serde_json::from_str::<Self>(&contents) else {
            return;
        };
        self.merge(user_config);
    }

    fn merge(&mut self, user_config: Self) {
        for view in user_config.views {
            if let Some(existing) = self
                .views
                .iter_mut()
                .find(|candidate| candidate.name.eq_ignore_ascii_case(&view.name))
            {
                *existing = view;
            } else {
                self.views.push(view);
            }
        }
        if user_config.auth != GitHubAuthConfig::default() {
            self.auth = user_config.auth;
        }
        if user_config.project_board != ProjectBoardConfig::default() {
            self.project_board = user_config.project_board;
        }
        if user_config.ui != UiEffectsConfig::default() {
            self.ui = user_config.ui;
        }
    }
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("tissues").join("config.json"))
}

fn repo_config_path() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .map(|path| path.join(".tissues").join("config.json"))
}
