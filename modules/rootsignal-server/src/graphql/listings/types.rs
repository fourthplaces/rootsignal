use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::graphql::context::Locale;
use crate::graphql::error;
use crate::graphql::loaders::{
    ContactsForLoader, EntityByIdLoader, LocationsForLoader, NotesForLoader, PolymorphicKey,
    SchedulesForLoader, ServiceByIdLoader, TagsForLoader, TranslationKey, TranslationLoader,
};

use super::super::contacts::types::GqlContact;
use super::super::entities::types::{GqlEntity, GqlService};
use super::super::locations::types::GqlLocation;
use super::super::notes::types::GqlNote;
use super::super::schedules::types::GqlSchedule;
use super::super::tags::types::GqlTag;

/// GraphQL listing type wrapping the raw Listing DB model.
#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct GqlListing {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub entity_id: Option<Uuid>,
    pub service_id: Option<Uuid>,
    pub source_url: Option<String>,
    pub location_text: Option<String>,
    pub in_language: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub freshness_score: f32,
    pub relevance_score: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<rootsignal_domains::listings::Listing> for GqlListing {
    fn from(l: rootsignal_domains::listings::Listing) -> Self {
        Self {
            id: l.id,
            title: l.title,
            description: l.description,
            status: l.status,
            entity_id: l.entity_id,
            service_id: l.service_id,
            source_url: l.source_url,
            location_text: l.location_text,
            in_language: l.in_language,
            expires_at: l.expires_at,
            freshness_score: l.freshness_score,
            relevance_score: l.relevance_score,
            created_at: l.created_at,
            updated_at: l.updated_at,
        }
    }
}

#[ComplexObject]
impl GqlListing {
    async fn entity(&self, ctx: &Context<'_>) -> Result<Option<GqlEntity>> {
        let Some(entity_id) = self.entity_id else {
            return Ok(None);
        };
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<EntityByIdLoader>>();
        Ok(loader.load_one(entity_id).await?)
    }

    async fn service(&self, ctx: &Context<'_>) -> Result<Option<GqlService>> {
        let Some(service_id) = self.service_id else {
            return Ok(None);
        };
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<ServiceByIdLoader>>();
        Ok(loader.load_one(service_id).await?)
    }

    async fn tags(&self, ctx: &Context<'_>) -> Result<Vec<GqlTag>> {
        let loader = ctx.data_unchecked::<async_graphql::dataloader::DataLoader<TagsForLoader>>();
        let key = PolymorphicKey("listing".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }

    async fn locations(&self, ctx: &Context<'_>) -> Result<Vec<GqlLocation>> {
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<LocationsForLoader>>();
        let key = PolymorphicKey("listing".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }

    async fn schedules(&self, ctx: &Context<'_>) -> Result<Vec<GqlSchedule>> {
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<SchedulesForLoader>>();
        let key = PolymorphicKey("listing".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }

    async fn contacts(&self, ctx: &Context<'_>) -> Result<Vec<GqlContact>> {
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<ContactsForLoader>>();
        let key = PolymorphicKey("listing".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }

    async fn notes(&self, ctx: &Context<'_>) -> Result<Vec<GqlNote>> {
        let loader = ctx.data_unchecked::<async_graphql::dataloader::DataLoader<NotesForLoader>>();
        let key = PolymorphicKey("listing".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }

    async fn cluster_siblings(&self, ctx: &Context<'_>) -> Result<Vec<GqlListing>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        // Single query: join through cluster_items to get sibling listings directly
        let siblings = sqlx::query_as::<_, rootsignal_domains::listings::Listing>(
            r#"
            SELECT l.*
            FROM cluster_items ci
            JOIN cluster_items ci2 ON ci2.cluster_id = ci.cluster_id AND ci2.item_type = 'listing'
            JOIN listings l ON l.id = ci2.item_id
            WHERE ci.item_type = 'listing' AND ci.item_id = $1
              AND ci2.item_id != $1
            ORDER BY ci2.similarity_score DESC NULLS LAST
            "#,
        )
        .bind(self.id)
        .fetch_all(pool)
        .await
        .map_err(|e| error::internal(e))?;
        Ok(siblings.into_iter().map(GqlListing::from).collect())
    }

    /// Translated title for the requested locale (with fallback chain).
    async fn translated_title(&self, ctx: &Context<'_>) -> Result<String> {
        let locale = ctx.data_unchecked::<Locale>();
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<TranslationLoader>>();

        // Try requested locale
        let key = TranslationKey {
            translatable_type: "listing".to_string(),
            translatable_id: self.id,
            field_name: "title".to_string(),
            locale: locale.0.clone(),
        };
        if let Some(content) = loader.load_one(key).await? {
            return Ok(content);
        }

        // Fallback to English
        if locale.0 != "en" {
            let en_key = TranslationKey {
                translatable_type: "listing".to_string(),
                translatable_id: self.id,
                field_name: "title".to_string(),
                locale: "en".to_string(),
            };
            if let Some(content) = loader.load_one(en_key).await? {
                return Ok(content);
            }
        }

        Ok(self.title.clone())
    }

    /// Translated description for the requested locale (with fallback chain).
    async fn translated_description(&self, ctx: &Context<'_>) -> Result<Option<String>> {
        let locale = ctx.data_unchecked::<Locale>();
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<TranslationLoader>>();

        // Try requested locale
        let key = TranslationKey {
            translatable_type: "listing".to_string(),
            translatable_id: self.id,
            field_name: "description".to_string(),
            locale: locale.0.clone(),
        };
        if let Some(content) = loader.load_one(key).await? {
            return Ok(Some(content));
        }

        // Fallback to English
        if locale.0 != "en" {
            let en_key = TranslationKey {
                translatable_type: "listing".to_string(),
                translatable_id: self.id,
                field_name: "description".to_string(),
                locale: "en".to_string(),
            };
            if let Some(content) = loader.load_one(en_key).await? {
                return Ok(Some(content));
            }
        }

        Ok(self.description.clone())
    }
}

/// Custom edge data for listing connections (carries geo-context when applicable).
#[derive(SimpleObject)]
pub struct GqlListingEdgeData {
    pub distance_miles: Option<f64>,
    pub zip_code: Option<String>,
    pub location_city: Option<String>,
}

impl GqlListingEdgeData {
    /// Create empty edge data (non-geo queries).
    pub fn empty() -> Self {
        Self {
            distance_miles: None,
            zip_code: None,
            location_city: None,
        }
    }
}
