use std::{fs, time::SystemTime};

use tissues::{
    app::{AssigneeFilter, IssueFilters, IssueSort, IssueStateFilter},
    cache::IssueCache,
    config::{AppConfig, GitHubAuthConfig, ProjectBoardConfig, SavedView},
    domain::{IssueState, IssueSummary},
    repo::Repository,
};

fn issue(number: u64, title: &str) -> IssueSummary {
    IssueSummary {
        number,
        title: title.to_string(),
        state: IssueState::Open,
        labels: Vec::new(),
        assignees: Vec::new(),
        author: None,
        created_at: None,
        updated_at: None,
        comment_count: 0,
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("tissues-{name}-{nanos}"))
}

#[test]
fn cache_round_trips_issue_lists_by_filter() {
    let root = temp_dir("cache-list");
    let cache = IssueCache::new(root.clone(), "owner/tissues".parse().unwrap());
    let filters = IssueFilters {
        state: IssueStateFilter::Open,
        assignee: AssigneeFilter::Me,
        labels: vec!["bug".to_string()],
        query: "panic".to_string(),
        sort: IssueSort::Comments,
    };

    cache
        .save_issues(&filters, &[issue(7, "Fix panic")])
        .expect("save issues");

    let loaded = cache.load_issues(&filters).expect("load issues");
    assert_eq!(loaded, Some(vec![issue(7, "Fix panic")]));

    fs::remove_dir_all(root).expect("remove temp cache");
}

#[test]
fn built_in_views_cover_common_triage_flows() {
    let config = AppConfig::with_builtin_views();

    let names = config
        .views
        .iter()
        .map(|view| view.name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"mine"));
    assert!(names.contains(&"untriaged"));
    assert!(names.contains(&"bugs"));
}

#[test]
fn saved_views_serialize_filters_for_user_config() {
    let config = AppConfig {
        views: vec![SavedView {
            name: "mine".to_string(),
            filters: IssueFilters {
                state: IssueStateFilter::Open,
                assignee: AssigneeFilter::Me,
                labels: Vec::new(),
                query: String::new(),
                sort: IssueSort::Updated,
            },
        }],
        auth: GitHubAuthConfig::default(),
        project_board: ProjectBoardConfig::default(),
    };

    let json = serde_json::to_string(&config).expect("serialize config");
    let parsed: AppConfig = serde_json::from_str(&json).expect("deserialize config");

    assert_eq!(parsed.views[0].name, "mine");
    assert_eq!(parsed.views[0].filters.assignee, AssigneeFilter::Me);
}

#[test]
fn project_board_config_serializes_owner_number_and_status_field() {
    let config = AppConfig {
        views: Vec::new(),
        auth: GitHubAuthConfig::default(),
        project_board: ProjectBoardConfig {
            owner: Some("owner".to_string()),
            number: Some(7),
            status_field: "Status".to_string(),
        },
    };

    let json = serde_json::to_string(&config).expect("serialize config");
    let parsed: AppConfig = serde_json::from_str(&json).expect("deserialize config");

    assert_eq!(parsed.project_board.owner.as_deref(), Some("owner"));
    assert_eq!(parsed.project_board.number, Some(7));
    assert_eq!(parsed.project_board.status_field, "Status");
}

#[test]
fn auth_config_serializes_github_cli_user() {
    let config = AppConfig {
        views: Vec::new(),
        auth: GitHubAuthConfig {
            gh_user: Some("Kade-Powell".to_string()),
        },
        project_board: ProjectBoardConfig::default(),
    };

    let json = serde_json::to_string(&config).expect("serialize config");
    let parsed: AppConfig = serde_json::from_str(&json).expect("deserialize config");

    assert_eq!(parsed.auth.gh_user.as_deref(), Some("Kade-Powell"));
}

#[test]
fn repo_config_overrides_user_config_for_repo_local_auth() {
    let root = temp_dir("config-merge");
    let user_path = root.join("user-config.json");
    let repo_path = root.join(".tissues").join("config.json");
    fs::create_dir_all(repo_path.parent().unwrap()).expect("create repo config dir");
    fs::write(
        &user_path,
        r#"{"auth":{"gh_user":"kpowel859_comcast"},"project_board":{"number":1}}"#,
    )
    .expect("write user config");
    fs::write(
        &repo_path,
        r#"{"auth":{"gh_user":"Kade-Powell"},"project_board":{"number":2}}"#,
    )
    .expect("write repo config");

    let config = AppConfig::load_from_paths(Some(user_path), Some(repo_path));

    assert_eq!(config.auth.gh_user.as_deref(), Some("Kade-Powell"));
    assert_eq!(config.project_board.number, Some(2));

    fs::remove_dir_all(root).expect("remove temp config");
}

#[test]
fn cache_repo_paths_are_safe_for_github_owner_repo_names() {
    let cache = IssueCache::new(
        temp_dir("cache-path"),
        Repository {
            owner: "Kade-Powell".to_string(),
            name: "tissues".to_string(),
        },
    );

    assert!(cache.repo_dir().ends_with("Kade-Powell__tissues"));
}
