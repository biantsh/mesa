pub const NOTICES: &str = include_str!("../runtime/notices.json");

pub fn links_to(url: &str) -> bool {
    url.starts_with("https://")
        && serde_json::to_string(url).is_ok_and(|quoted| NOTICES.contains(&format!("\"url\":{quoted}")))
}
