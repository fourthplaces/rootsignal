use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::graphql::context::Locale;
use crate::graphql::error;
use crate::graphql::loaders::*;

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
    pub source_locale: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub freshness_score: f32,
    pub relevance_score: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<taproot_domains::listings::Listing> for GqlListing {
    fn from(l: taproot_domains::listings::Listing) -> Self {
        Self {
            id: l.id,
            title: l.title,
            description: l.description,
            status: l.status,
            entity_id: l.entity_id,
            service_id: l.service_id,
            source_url: l.source_url,
            location_text: l.location_text,
            source_locale: l.source_locale,
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
        let loader = ctx.data_unchecked::<async_graphql::dataloader::DataLoader<EntityByIdLoader>>();
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
        let siblings = taproot_domains::listings::ListingDetail::cluster_siblings(self.id, pool)
            .await
            .unwrap_or_default();
        // Convert ListingDetail to GqlListing by re-fetching raw listings
        // (cluster siblings are small cardinality, so this is fine)
        let ids: Vec<Uuid> = siblings.iter().map(|s| s.id).collect();
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let listings = sqlx::query_as::<_, taproot_domains::listings::Listing>(
            "SELECT * FROM listings WHERE id = ANY($1)",
        )
        .bind(&ids)
        .fetch_all(pool)
        .await
        .map_err(|e| error::internal(e))?;
        Ok(listings.into_iter().map(GqlListing::from).collect())
    }

    /// Translated title for the requested locale (with fallback chain).
    async fn translated_title(&self, ctx: &Context<'_>) -> Result<String> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let locale = ctx.data_unchecked::<Locale>();
        let translated = sqlx::query_as::<_, (String,)>(
            r#"SELECT COALESCE(
                (SELECT content FROM translations WHERE translatable_type = 'listing' AND translatable_id = $1 AND field_name = 'title' AND locale = $2),
                (SELECT content FROM translations WHERE translatable_type = 'listing' AND translatable_id = $1 AND field_name = 'title' AND locale = 'en'),
                $3
            )"#,
        )
        .bind(self.id)
        .bind(&locale.0)
        .bind(&self.title)
        .fetch_one(pool)
        .await
        .map_err(|e| error::internal(e))?;
        Ok(translated.0)
    }

    /// Translated description for the requested locale (with fallback chain).
    async fn translated_description(&self, ctx: &Context<'_>) -> Result<Option<String>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let locale = ctx.data_unchecked::<Locale>();
        let translated = sqlx::query_as::<_, (Option<String>,)>(
            r#"SELECT COALESCE(
                (SELECT content FROM translations WHERE translatable_type = 'listing' AND translatable_id = $1 AND field_name = 'description' AND locale = $2),
                (SELECT content FROM translations WHERE translatable_type = 'listing' AND translatable_id = $1 AND field_name = 'description' AND locale = 'en'),
                $3
            )"#,
        )
        .bind(self.id)
        .bind(&locale.0)
        .bind(&self.description)
        .fetch_one(pool)
        .await
        .map_err(|e| error::internal(e))?;
        Ok(translated.0)
    }
}

/// Custom edge data for listing connections (carries geo-context when applicable).
#[derive(SimpleObject)]
pub struct GqlListingEdgeData {
    pub distance_miles: Option<f64>,
    pub zip_code: Option<String>,
    pub location_city: Option<String>,
}
