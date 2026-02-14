use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(SimpleObject, Clone)]
pub struct GqlContact {
    pub id: Uuid,
    pub name: Option<String>,
    pub title: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub department: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<rootsignal_domains::entities::Contact> for GqlContact {
    fn from(c: rootsignal_domains::entities::Contact) -> Self {
        Self {
            id: c.id,
            name: c.name,
            title: c.title,
            email: c.email,
            phone: c.phone,
            department: c.department,
            created_at: c.created_at,
        }
    }
}
