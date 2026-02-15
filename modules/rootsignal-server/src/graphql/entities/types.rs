use async_graphql::*;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::graphql::loaders::*;

use super::super::contacts::types::GqlContact;
use super::super::locations::types::GqlLocation;
use super::super::notes::types::GqlNote;
use super::super::schedules::types::GqlSchedule;
use super::super::sources::types::GqlSource;
use super::super::tags::types::GqlTag;

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct GqlEntity {
    pub id: Uuid,
    pub name: String,
    pub entity_type: String,
    pub description: Option<String>,
    pub website: Option<String>,
    pub telephone: Option<String>,
    pub email: Option<String>,
    pub verified: bool,
    pub in_language: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<rootsignal_domains::entities::Entity> for GqlEntity {
    fn from(e: rootsignal_domains::entities::Entity) -> Self {
        Self {
            id: e.id,
            name: e.name,
            entity_type: e.entity_type,
            description: e.description,
            website: e.website,
            telephone: e.telephone,
            email: e.email,
            verified: e.verified,
            in_language: e.in_language,
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

#[ComplexObject]
impl GqlEntity {
    async fn tags(&self, ctx: &Context<'_>) -> Result<Vec<GqlTag>> {
        let loader = ctx.data_unchecked::<async_graphql::dataloader::DataLoader<TagsForLoader>>();
        let key = PolymorphicKey("entity".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }

    async fn locations(&self, ctx: &Context<'_>) -> Result<Vec<GqlLocation>> {
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<LocationsForLoader>>();
        let key = PolymorphicKey("entity".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }

    async fn contacts(&self, ctx: &Context<'_>) -> Result<Vec<GqlContact>> {
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<ContactsForLoader>>();
        let key = PolymorphicKey("entity".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }

    async fn schedules(&self, ctx: &Context<'_>) -> Result<Vec<GqlSchedule>> {
        let loader =
            ctx.data_unchecked::<async_graphql::dataloader::DataLoader<SchedulesForLoader>>();
        let key = PolymorphicKey("entity".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }

    async fn notes(&self, ctx: &Context<'_>) -> Result<Vec<GqlNote>> {
        let loader = ctx.data_unchecked::<async_graphql::dataloader::DataLoader<NotesForLoader>>();
        let key = PolymorphicKey("entity".to_string(), self.id);
        Ok(loader.load_one(key).await?.unwrap_or_default())
    }

    /// Number of signals associated with this entity.
    async fn signal_count(&self, ctx: &Context<'_>) -> Result<i32> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM signals WHERE entity_id = $1",
        )
        .bind(self.id)
        .fetch_one(pool)
        .await
        .unwrap_or((0,));
        Ok(row.0 as i32)
    }

    async fn sources(&self, ctx: &Context<'_>) -> Result<Vec<GqlSource>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let sources =
            rootsignal_domains::scraping::Source::find_by_entity_id(self.id, pool)
                .await
                .unwrap_or_default();
        Ok(sources.into_iter().map(GqlSource::from).collect())
    }

    async fn services(&self, ctx: &Context<'_>) -> Result<Vec<GqlService>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let services = rootsignal_domains::shared::Service::find_by_entity_id(self.id, pool)
            .await
            .unwrap_or_default();
        Ok(services.into_iter().map(GqlService::from).collect())
    }

    async fn listings(
        &self,
        ctx: &Context<'_>,
    ) -> Result<Vec<super::super::listings::types::GqlListing>> {
        let pool = ctx.data_unchecked::<sqlx::PgPool>();
        let listings = sqlx::query_as::<_, rootsignal_domains::listings::Listing>(
            "SELECT * FROM listings WHERE entity_id = $1 AND status = 'active' ORDER BY created_at DESC",
        )
        .bind(self.id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
        Ok(listings
            .into_iter()
            .map(super::super::listings::types::GqlListing::from)
            .collect())
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlService {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub url: Option<String>,
    pub email: Option<String>,
    pub telephone: Option<String>,
    pub interpretation_services: Option<String>,
    pub application_process: Option<String>,
    pub price_range: Option<String>,
    pub eligibility: Option<String>,
    pub in_language: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<rootsignal_domains::shared::Service> for GqlService {
    fn from(s: rootsignal_domains::shared::Service) -> Self {
        Self {
            id: s.id,
            entity_id: s.entity_id,
            name: s.name,
            description: s.description,
            status: s.status,
            url: s.url,
            email: s.email,
            telephone: s.telephone,
            interpretation_services: s.interpretation_services,
            application_process: s.application_process,
            price_range: s.price_range,
            eligibility: s.eligibility,
            in_language: s.in_language,
            created_at: s.created_at,
            updated_at: s.updated_at,
        }
    }
}
