use crate::listings::ListingFilters;

/// Append tag EXISTS subqueries for all set tag filters.
/// `taggable_type_alias` is the SQL alias for the table whose `.id` column
/// should be matched (e.g., "l" for `listings l`).
pub fn append_tag_filters(
    qb: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    filters: &ListingFilters,
    taggable_type_alias: &str,
) {
    let tag_filters: Vec<(&str, &Option<String>)> = vec![
        ("signal_domain", &filters.signal_domain),
        ("audience_role", &filters.audience_role),
        ("category", &filters.category),
        ("listing_type", &filters.listing_type),
        ("urgency", &filters.urgency),
        ("confidence", &filters.confidence),
        ("capacity_status", &filters.capacity_status),
        ("radius_relevant", &filters.radius_relevant),
        ("population", &filters.population),
    ];

    for (kind, value) in &tag_filters {
        if let Some(val) = value {
            qb.push(format!(
                "AND EXISTS (SELECT 1 FROM taggables tg JOIN tags t ON t.id = tg.tag_id \
                 WHERE tg.taggable_type = 'listing' AND tg.taggable_id = {taggable_type_alias}.id AND t.kind = "
            ));
            qb.push_bind(*kind);
            qb.push(" AND t.value = ");
            qb.push_bind(val.clone());
            qb.push(") ");
        }
    }
}

/// Append tag EXISTS subqueries from a slice of (kind, Option<String>) pairs.
/// Used when the caller has already constructed the filter tuples.
pub fn append_tag_filters_from_slice<'a>(
    qb: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>,
    filters: &[(&'a str, Option<String>)],
    taggable_type_alias: &str,
) {
    for (kind, value) in filters {
        if let Some(val) = value {
            qb.push(format!(
                "AND EXISTS (SELECT 1 FROM taggables tg JOIN tags t ON t.id = tg.tag_id \
                 WHERE tg.taggable_type = 'listing' AND tg.taggable_id = {taggable_type_alias}.id AND t.kind = "
            ));
            qb.push_bind(*kind);
            qb.push(" AND t.value = ");
            qb.push_bind(val.clone());
            qb.push(") ");
        }
    }
}

/// Append LEFT JOIN clauses for translation fallback (requested locale → English → source).
/// Adds `t_title`, `t_desc`, `en_title`, `en_desc` aliases.
/// `table_alias` is the SQL alias for the listings table (e.g., "l").
pub fn append_translation_joins(
    qb: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    locale: &str,
    table_alias: &str,
) {
    qb.push(format!(
        "LEFT JOIN translations t_title ON t_title.translatable_type = 'listing' \
         AND t_title.translatable_id = {table_alias}.id AND t_title.field_name = 'title' AND t_title.locale = "
    ));
    qb.push_bind(locale.to_string());
    qb.push(" ");
    qb.push(format!(
        "LEFT JOIN translations t_desc ON t_desc.translatable_type = 'listing' \
         AND t_desc.translatable_id = {table_alias}.id AND t_desc.field_name = 'description' AND t_desc.locale = "
    ));
    qb.push_bind(locale.to_string());
    qb.push(" ");
    qb.push(format!(
        "LEFT JOIN translations en_title ON en_title.translatable_type = 'listing' \
         AND en_title.translatable_id = {table_alias}.id AND en_title.field_name = 'title' AND en_title.locale = 'en' "
    ));
    qb.push(format!(
        "LEFT JOIN translations en_desc ON en_desc.translatable_type = 'listing' \
         AND en_desc.translatable_id = {table_alias}.id AND en_desc.field_name = 'description' AND en_desc.locale = 'en' "
    ));
}
