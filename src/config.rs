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
    pub views: Vec<SavedView>,
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
        }
    }

    pub fn load() -> Self {
        let mut config = Self::with_builtin_views();
        let Some(path) = config_path() else {
            return config;
        };
        let Ok(contents) = fs::read_to_string(path) else {
            return config;
        };
        let Ok(user_config) = serde_json::from_str::<Self>(&contents) else {
            return config;
        };

        for view in user_config.views {
            if let Some(existing) = config
                .views
                .iter_mut()
                .find(|candidate| candidate.name.eq_ignore_ascii_case(&view.name))
            {
                *existing = view;
            } else {
                config.views.push(view);
            }
        }

        config
    }
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("tissues").join("config.json"))
}
