use async_graphql::dataloader::Loader;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use super::contacts::types::GqlContact;
use super::entities::types::{GqlEntity, GqlService};
use super::locations::types::GqlLocation;
use super::notes::types::GqlNote;
use super::schedules::types::GqlSchedule;
use super::tags::types::GqlTag;

/// Composite key for polymorphic associations: (type_discriminator, id).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PolymorphicKey(pub String, pub Uuid);

// ─── Entity by ID ────────────────────────────────────────────────────────────

pub struct EntityByIdLoader {
    pub pool: sqlx::PgPool,
}

impl Loader<Uuid> for EntityByIdLoader {
    type Value = GqlEntity;
    type Error = Arc<sqlx::Error>;

    async fn load(&self, keys: &[Uuid]) -> Result<HashMap<Uuid, Self::Value>, Self::Error> {
        let rows = sqlx::query_as::<_, taproot_domains::entities::Entity>(
            "SELECT * FROM entities WHERE id = ANY($1)",
        )
        .bind(keys)
        .fetch_all(&self.pool)
        .await
        .map_err(Arc::new)?;

        Ok(rows
            .into_iter()
            .map(|e| (e.id, GqlEntity::from(e)))
            .collect())
    }
}

// ─── Service by ID ───────────────────────────────────────────────────────────

pub struct ServiceByIdLoader {
    pub pool: sqlx::PgPool,
}

impl Loader<Uuid> for ServiceByIdLoader {
    type Value = GqlService;
    type Error = Arc<sqlx::Error>;

    async fn load(&self, keys: &[Uuid]) -> Result<HashMap<Uuid, Self::Value>, Self::Error> {
        let rows = sqlx::query_as::<_, taproot_domains::entities::Service>(
            "SELECT * FROM services WHERE id = ANY($1)",
        )
        .bind(keys)
        .fetch_all(&self.pool)
        .await
        .map_err(Arc::new)?;

        Ok(rows
            .into_iter()
            .map(|s| (s.id, GqlService::from(s)))
            .collect())
    }
}

// ─── Tags for (type, id) ────────────────────────────────────────────────────

pub struct TagsForLoader {
    pub pool: sqlx::PgPool,
}

#[derive(sqlx::FromRow)]
struct TagWithOwner {
    #[sqlx(flatten)]
    tag: taproot_domains::entities::Tag,
    taggable_type: String,
    taggable_id: Uuid,
}

impl Loader<PolymorphicKey> for TagsForLoader {
    type Value = Vec<GqlTag>;
    type Error = Arc<sqlx::Error>;

    async fn load(
        &self,
        keys: &[PolymorphicKey],
    ) -> Result<HashMap<PolymorphicKey, Self::Value>, Self::Error> {
        let types: Vec<&str> = keys.iter().map(|k| k.0.as_str()).collect();
        let ids: Vec<Uuid> = keys.iter().map(|k| k.1).collect();

        let rows = sqlx::query_as::<_, TagWithOwner>(
            r#"SELECT t.*, tb.taggable_type, tb.taggable_id
               FROM tags t
               JOIN taggables tb ON tb.tag_id = t.id
               WHERE tb.taggable_type = ANY($1) AND tb.taggable_id = ANY($2)"#,
        )
        .bind(&types)
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(Arc::new)?;

        let mut map: HashMap<PolymorphicKey, Vec<GqlTag>> = HashMap::new();
        for row in rows {
            let key = PolymorphicKey(row.taggable_type, row.taggable_id);
            map.entry(key)
                .or_default()
                .push(GqlTag::from(row.tag));
        }
        Ok(map)
    }
}

// ─── Locations for (type, id) ────────────────────────────────────────────────

pub struct LocationsForLoader {
    pub pool: sqlx::PgPool,
}

#[derive(sqlx::FromRow)]
struct LocationWithOwner {
    #[sqlx(flatten)]
    location: taproot_domains::entities::Location,
    locatable_type: String,
    locatable_id: Uuid,
}

impl Loader<PolymorphicKey> for LocationsForLoader {
    type Value = Vec<GqlLocation>;
    type Error = Arc<sqlx::Error>;

    async fn load(
        &self,
        keys: &[PolymorphicKey],
    ) -> Result<HashMap<PolymorphicKey, Self::Value>, Self::Error> {
        let types: Vec<&str> = keys.iter().map(|k| k.0.as_str()).collect();
        let ids: Vec<Uuid> = keys.iter().map(|k| k.1).collect();

        let rows = sqlx::query_as::<_, LocationWithOwner>(
            r#"SELECT l.*, la.locatable_type, la.locatable_id
               FROM locations l
               JOIN locationables la ON la.location_id = l.id
               WHERE la.locatable_type = ANY($1) AND la.locatable_id = ANY($2)"#,
        )
        .bind(&types)
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(Arc::new)?;

        let mut map: HashMap<PolymorphicKey, Vec<GqlLocation>> = HashMap::new();
        for row in rows {
            let key = PolymorphicKey(row.locatable_type, row.locatable_id);
            map.entry(key)
                .or_default()
                .push(GqlLocation::from(row.location));
        }
        Ok(map)
    }
}

// ─── Schedules for (type, id) ────────────────────────────────────────────────

pub struct SchedulesForLoader {
    pub pool: sqlx::PgPool,
}

impl Loader<PolymorphicKey> for SchedulesForLoader {
    type Value = Vec<GqlSchedule>;
    type Error = Arc<sqlx::Error>;

    async fn load(
        &self,
        keys: &[PolymorphicKey],
    ) -> Result<HashMap<PolymorphicKey, Self::Value>, Self::Error> {
        let types: Vec<&str> = keys.iter().map(|k| k.0.as_str()).collect();
        let ids: Vec<Uuid> = keys.iter().map(|k| k.1).collect();

        let rows = sqlx::query_as::<_, taproot_domains::entities::Schedule>(
            "SELECT * FROM schedules WHERE scheduleable_type = ANY($1) AND scheduleable_id = ANY($2)",
        )
        .bind(&types)
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(Arc::new)?;

        let mut map: HashMap<PolymorphicKey, Vec<GqlSchedule>> = HashMap::new();
        for row in rows {
            let key = PolymorphicKey(row.scheduleable_type.clone(), row.scheduleable_id);
            map.entry(key)
                .or_default()
                .push(GqlSchedule::from(row));
        }
        Ok(map)
    }
}

// ─── Contacts for (type, id) ────────────────────────────────────────────────

pub struct ContactsForLoader {
    pub pool: sqlx::PgPool,
}

impl Loader<PolymorphicKey> for ContactsForLoader {
    type Value = Vec<GqlContact>;
    type Error = Arc<sqlx::Error>;

    async fn load(
        &self,
        keys: &[PolymorphicKey],
    ) -> Result<HashMap<PolymorphicKey, Self::Value>, Self::Error> {
        let types: Vec<&str> = keys.iter().map(|k| k.0.as_str()).collect();
        let ids: Vec<Uuid> = keys.iter().map(|k| k.1).collect();

        let rows = sqlx::query_as::<_, taproot_domains::entities::Contact>(
            "SELECT * FROM contacts WHERE contactable_type = ANY($1) AND contactable_id = ANY($2)",
        )
        .bind(&types)
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(Arc::new)?;

        let mut map: HashMap<PolymorphicKey, Vec<GqlContact>> = HashMap::new();
        for row in rows {
            let key = PolymorphicKey(row.contactable_type.clone(), row.contactable_id);
            map.entry(key)
                .or_default()
                .push(GqlContact::from(row));
        }
        Ok(map)
    }
}

// ─── Notes for (type, id) ───────────────────────────────────────────────────

pub struct NotesForLoader {
    pub pool: sqlx::PgPool,
}

impl Loader<PolymorphicKey> for NotesForLoader {
    type Value = Vec<GqlNote>;
    type Error = Arc<sqlx::Error>;

    async fn load(
        &self,
        keys: &[PolymorphicKey],
    ) -> Result<HashMap<PolymorphicKey, Self::Value>, Self::Error> {
        let types: Vec<&str> = keys.iter().map(|k| k.0.as_str()).collect();
        let ids: Vec<Uuid> = keys.iter().map(|k| k.1).collect();

        let rows = sqlx::query_as::<_, taproot_domains::entities::Note>(
            r#"SELECT n.* FROM notes n
               JOIN noteables nb ON nb.note_id = n.id
               WHERE nb.noteable_type = ANY($1) AND nb.noteable_id = ANY($2)
                 AND (n.expired_at IS NULL OR n.expired_at > NOW())
               ORDER BY n.created_at DESC"#,
        )
        .bind(&types)
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(Arc::new)?;

        // Notes don't carry their owner — re-query noteables to map them back.
        // This is a pragmatic shortcut; ideally we'd join in the query above.
        // For now, we batch all notes and re-fetch the mapping.
        let note_ids: Vec<Uuid> = rows.iter().map(|n| n.id).collect();
        let mappings = sqlx::query_as::<_, (Uuid, String, Uuid)>(
            "SELECT note_id, noteable_type, noteable_id FROM noteables WHERE note_id = ANY($1)",
        )
        .bind(&note_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(Arc::new)?;

        let note_map: HashMap<Uuid, taproot_domains::entities::Note> =
            rows.into_iter().map(|n| (n.id, n)).collect();

        let mut map: HashMap<PolymorphicKey, Vec<GqlNote>> = HashMap::new();
        for (note_id, noteable_type, noteable_id) in mappings {
            let key = PolymorphicKey(noteable_type, noteable_id);
            if let Some(note) = note_map.get(&note_id) {
                map.entry(key)
                    .or_default()
                    .push(GqlNote::from(note.clone()));
            }
        }
        Ok(map)
    }
}
