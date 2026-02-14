-- Tag kinds: self-describing taxonomy dimensions that power dynamic AI prompt instructions.
-- Adding a new classification dimension requires only seeding a tag_kind + its tag values.

CREATE TABLE IF NOT EXISTS tag_kinds (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    description TEXT,
    allowed_resource_types TEXT[] NOT NULL DEFAULT '{}',
    required BOOLEAN NOT NULL DEFAULT FALSE,
    is_public BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Seed tag_kinds for listings
INSERT INTO tag_kinds (slug, display_name, required, description, allowed_resource_types) VALUES
    ('listing_type', 'Signal Type', TRUE, 'Classification of the signal', '{listing}'),
    ('audience_role', 'Audience Role', TRUE, 'Who can act on this', '{listing}'),
    ('category', 'Category', FALSE, 'Subject area', '{listing}'),
    ('signal_domain', 'Signal Domain', FALSE, 'Broad domain grouping', '{listing}'),
    ('urgency', 'Urgency', FALSE, 'Time sensitivity', '{listing}'),
    ('confidence', 'Confidence', FALSE, 'Extraction confidence level', '{listing}'),
    ('capacity_status', 'Capacity', FALSE, 'Current capacity status', '{listing}'),
    ('radius_relevant', 'Geographic Scope', FALSE, 'How far this signal carries', '{listing}'),
    ('population', 'Population Served', FALSE, 'Target populations', '{listing}')
ON CONFLICT (slug) DO NOTHING;

-- ── New audience roles ──────────────────────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('audience_role', 'skilled_professional', 'Skilled Professional'),
    ('audience_role', 'citizen_scientist', 'Citizen Scientist'),
    ('audience_role', 'land_steward', 'Land Steward'),
    ('audience_role', 'conscious_consumer', 'Conscious Consumer'),
    ('audience_role', 'educator', 'Educator')
ON CONFLICT (kind, value) DO NOTHING;

-- ── Ecological listing types ────────────────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('listing_type', 'habitat_restoration', 'Habitat Restoration'),
    ('listing_type', 'species_monitoring', 'Species Monitoring'),
    ('listing_type', 'water_quality_alert', 'Water Quality Alert'),
    ('listing_type', 'invasive_species_removal', 'Invasive Species Removal'),
    ('listing_type', 'tree_planting', 'Tree Planting'),
    ('listing_type', 'wildlife_survey', 'Wildlife Survey'),
    ('listing_type', 'conservation_easement', 'Conservation Easement'),
    ('listing_type', 'pollinator_habitat', 'Pollinator Habitat'),
    ('listing_type', 'soil_health_assessment', 'Soil Health Assessment'),
    ('listing_type', 'climate_adaptation', 'Climate Adaptation'),
    ('listing_type', 'watershed_cleanup', 'Watershed Cleanup'),
    ('listing_type', 'ecological_survey', 'Ecological Survey')
ON CONFLICT (kind, value) DO NOTHING;

-- ── Civic listing types ─────────────────────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('listing_type', 'public_comment_period', 'Public Comment Period'),
    ('listing_type', 'zoning_hearing', 'Zoning Hearing'),
    ('listing_type', 'budget_hearing', 'Budget Hearing'),
    ('listing_type', 'election_info', 'Election Information'),
    ('listing_type', 'civic_volunteer', 'Civic Volunteer Opportunity')
ON CONFLICT (kind, value) DO NOTHING;

-- ── Human services listing types ────────────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('listing_type', 'food_distribution', 'Food Distribution'),
    ('listing_type', 'shelter_availability', 'Shelter Availability'),
    ('listing_type', 'health_screening', 'Health Screening'),
    ('listing_type', 'benefits_enrollment', 'Benefits Enrollment'),
    ('listing_type', 'crisis_hotline', 'Crisis Hotline'),
    ('listing_type', 'support_group', 'Support Group'),
    ('listing_type', 'legal_clinic', 'Legal Clinic'),
    ('listing_type', 'transportation_assistance', 'Transportation Assistance'),
    ('listing_type', 'utility_assistance', 'Utility Assistance')
ON CONFLICT (kind, value) DO NOTHING;

-- ── Knowledge / awareness listing types ─────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('listing_type', 'research_opportunity', 'Research Opportunity'),
    ('listing_type', 'educational_program', 'Educational Program'),
    ('listing_type', 'data_release', 'Data Release'),
    ('listing_type', 'public_lecture', 'Public Lecture'),
    ('listing_type', 'certification_program', 'Certification Program')
ON CONFLICT (kind, value) DO NOTHING;

-- ── Ecological categories ───────────────────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('category', 'water_quality', 'Water Quality'),
    ('category', 'air_quality', 'Air Quality'),
    ('category', 'soil_health', 'Soil Health'),
    ('category', 'biodiversity', 'Biodiversity'),
    ('category', 'pollinators', 'Pollinators'),
    ('category', 'urban_forestry', 'Urban Forestry'),
    ('category', 'wetlands', 'Wetlands'),
    ('category', 'prairies', 'Prairies'),
    ('category', 'invasive_species', 'Invasive Species'),
    ('category', 'wildlife', 'Wildlife'),
    ('category', 'climate_resilience', 'Climate Resilience'),
    ('category', 'green_infrastructure', 'Green Infrastructure'),
    ('category', 'energy', 'Energy'),
    ('category', 'waste_reduction', 'Waste Reduction'),
    ('category', 'sustainable_agriculture', 'Sustainable Agriculture'),
    ('category', 'conservation', 'Conservation'),
    ('category', 'watershed', 'Watershed'),
    ('category', 'land_use', 'Land Use'),
    ('category', 'noise_pollution', 'Noise Pollution')
ON CONFLICT (kind, value) DO NOTHING;

-- ── Civic categories ────────────────────────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('category', 'zoning', 'Zoning'),
    ('category', 'budgets', 'Budgets'),
    ('category', 'elections', 'Elections'),
    ('category', 'public_safety', 'Public Safety'),
    ('category', 'infrastructure', 'Infrastructure'),
    ('category', 'parks_recreation', 'Parks & Recreation'),
    ('category', 'economic_development', 'Economic Development'),
    ('category', 'small_business', 'Small Business'),
    ('category', 'cooperative', 'Cooperative'),
    ('category', 'workforce_development', 'Workforce Development'),
    ('category', 'public_transit', 'Public Transit'),
    ('category', 'digital_equity', 'Digital Equity')
ON CONFLICT (kind, value) DO NOTHING;

-- ── Crisis categories ───────────────────────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('category', 'weather_emergency', 'Weather Emergency'),
    ('category', 'flood', 'Flood'),
    ('category', 'fire', 'Fire'),
    ('category', 'public_health_emergency', 'Public Health Emergency'),
    ('category', 'infrastructure_failure', 'Infrastructure Failure'),
    ('category', 'evacuation', 'Evacuation'),
    ('category', 'missing_person', 'Missing Person'),
    ('category', 'amber_alert', 'Amber Alert'),
    ('category', 'boil_water', 'Boil Water Advisory')
ON CONFLICT (kind, value) DO NOTHING;

-- ── Human needs gap categories ──────────────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('category', 'food_desert', 'Food Desert'),
    ('category', 'healthcare_gap', 'Healthcare Gap'),
    ('category', 'shelter_shortage', 'Shelter Shortage'),
    ('category', 'childcare_desert', 'Childcare Desert'),
    ('category', 'transit_desert', 'Transit Desert'),
    ('category', 'digital_divide', 'Digital Divide')
ON CONFLICT (kind, value) DO NOTHING;

-- ── Fix signal domains (add missing, rename misnamed) ───────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('signal_domain', 'ecological_stewardship', 'Ecological Stewardship'),
    ('signal_domain', 'knowledge_awareness', 'Knowledge & Awareness')
ON CONFLICT (kind, value) DO NOTHING;

-- Rename cultural_ecological → ecological_stewardship (if exists)
UPDATE tags SET value = 'ecological_stewardship', display_name = 'Ecological Stewardship'
WHERE kind = 'signal_domain' AND value = 'cultural_ecological';

-- ── Urgency tags ────────────────────────────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('urgency', 'immediate', 'Immediate'),
    ('urgency', 'this_week', 'This Week'),
    ('urgency', 'this_month', 'This Month'),
    ('urgency', 'ongoing', 'Ongoing'),
    ('urgency', 'flexible', 'Flexible')
ON CONFLICT (kind, value) DO NOTHING;

-- ── Confidence tags ─────────────────────────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('confidence', 'high', 'High'),
    ('confidence', 'medium', 'Medium'),
    ('confidence', 'low', 'Low')
ON CONFLICT (kind, value) DO NOTHING;

-- ── Capacity status tags ────────────────────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('capacity_status', 'accepting', 'Accepting'),
    ('capacity_status', 'limited', 'Limited'),
    ('capacity_status', 'at_capacity', 'At Capacity'),
    ('capacity_status', 'unknown', 'Unknown')
ON CONFLICT (kind, value) DO NOTHING;

-- ── Radius relevant tags ────────────────────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('radius_relevant', 'neighborhood', 'Neighborhood'),
    ('radius_relevant', 'city', 'City'),
    ('radius_relevant', 'metro', 'Metro'),
    ('radius_relevant', 'region', 'Region'),
    ('radius_relevant', 'national', 'National'),
    ('radius_relevant', 'global', 'Global')
ON CONFLICT (kind, value) DO NOTHING;

-- ── Population tags ─────────────────────────────────────────────────────────

INSERT INTO tags (kind, value, display_name) VALUES
    ('population', 'youth', 'Youth'),
    ('population', 'seniors', 'Seniors'),
    ('population', 'families', 'Families'),
    ('population', 'immigrants', 'Immigrants'),
    ('population', 'refugees', 'Refugees'),
    ('population', 'veterans', 'Veterans'),
    ('population', 'unhoused', 'Unhoused'),
    ('population', 'disabled', 'Disabled'),
    ('population', 'lgbtq', 'LGBTQ+'),
    ('population', 'indigenous', 'Indigenous')
ON CONFLICT (kind, value) DO NOTHING;
