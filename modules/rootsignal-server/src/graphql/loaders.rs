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
        let rows = sqlx::query_as::<_, rootsignal_domains::entities::Entity>(
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
        let rows = sqlx::query_as::<_, rootsignal_domains::shared::Service>(
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
    tag: rootsignal_domains::taxonomy::Tag,
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
    location: rootsignal_domains::geo::Location,
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

        let rows = sqlx::query_as::<_, rootsignal_domains::shared::Schedule>(
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

        let rows = sqlx::query_as::<_, rootsignal_domains::shared::Contact>(
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

// ─── Translation for (type, id, field, locale) ──────────────────────────────

/// Key for translation lookups: (translatable_type, translatable_id, field_name, locale).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TranslationKey {
    pub translatable_type: String,
    pub translatable_id: Uuid,
    pub field_name: String,
    pub locale: String,
}

pub struct TranslationLoader {
    pub pool: sqlx::PgPool,
}

impl Loader<TranslationKey> for TranslationLoader {
    type Value = String;
    type Error = Arc<sqlx::Error>;

    async fn load(
        &self,
        keys: &[TranslationKey],
    ) -> Result<HashMap<TranslationKey, Self::Value>, Self::Error> {
        let types: Vec<&str> = keys.iter().map(|k| k.translatable_type.as_str()).collect();
        let ids: Vec<Uuid> = keys.iter().map(|k| k.translatable_id).collect();
        let fields: Vec<&str> = keys.iter().map(|k| k.field_name.as_str()).collect();
        let locales: Vec<&str> = keys.iter().map(|k| k.locale.as_str()).collect();

        let rows = sqlx::query_as::<_, (String, Uuid, String, String, String)>(
            r#"SELECT translatable_type, translatable_id, field_name, locale, content
               FROM translations
               WHERE translatable_type = ANY($1)
                 AND translatable_id = ANY($2)
                 AND field_name = ANY($3)
                 AND locale = ANY($4)"#,
        )
        .bind(&types)
        .bind(&ids)
        .bind(&fields)
        .bind(&locales)
        .fetch_all(&self.pool)
        .await
        .map_err(Arc::new)?;

        let mut map = HashMap::new();
        for (ttype, tid, fname, locale, content) in rows {
            let key = TranslationKey {
                translatable_type: ttype,
                translatable_id: tid,
                field_name: fname,
                locale,
            };
            map.insert(key, content);
        }
        Ok(map)
    }
}

// ─── Notes for (type, id) ───────────────────────────────────────────────────

pub struct NotesForLoader {
    pub pool: sqlx::PgPool,
}

#[derive(sqlx::FromRow)]
struct NoteWithOwner {
    #[sqlx(flatten)]
    note: rootsignal_domains::shared::Note,
    noteable_type: String,
    noteable_id: Uuid,
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

        let rows = sqlx::query_as::<_, NoteWithOwner>(
            r#"SELECT n.*, nb.noteable_type, nb.noteable_id
               FROM notes n
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

        let mut map: HashMap<PolymorphicKey, Vec<GqlNote>> = HashMap::new();
        for row in rows {
            let key = PolymorphicKey(row.noteable_type, row.noteable_id);
            map.entry(key)
                .or_default()
                .push(GqlNote::from(row.note));
        }
        Ok(map)
    }
}
