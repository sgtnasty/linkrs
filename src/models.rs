use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Link {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub date_modified: String,
}

#[derive(Debug, Deserialize)]
pub struct LinkInput {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginInput {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct CurrentUser {
    pub username: String,
}
