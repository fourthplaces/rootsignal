-- Seed initial tags for the taxonomy

-- Listing types
INSERT INTO tags (kind, value, display_name) VALUES
    ('listing_type', 'volunteer_opportunity', 'Volunteer Opportunity'),
    ('listing_type', 'mutual_aid', 'Mutual Aid'),
    ('listing_type', 'community_event', 'Community Event'),
    ('listing_type', 'public_meeting', 'Public Meeting'),
    ('listing_type', 'resource_available', 'Resource Available'),
    ('listing_type', 'service_available', 'Service Available'),
    ('listing_type', 'job_opportunity', 'Job Opportunity'),
    ('listing_type', 'community_alert', 'Community Alert'),
    ('listing_type', 'advocacy_action', 'Advocacy Action'),
    ('listing_type', 'fundraiser', 'Fundraiser'),
    ('listing_type', 'training', 'Training / Workshop')
ON CONFLICT (kind, value) DO NOTHING;

-- Audience roles
INSERT INTO tags (kind, value, display_name) VALUES
    ('audience_role', 'volunteer', 'Volunteer'),
    ('audience_role', 'donor', 'Donor'),
    ('audience_role', 'recipient', 'Recipient / Client'),
    ('audience_role', 'advocate', 'Advocate'),
    ('audience_role', 'participant', 'Participant'),
    ('audience_role', 'attendee', 'Attendee'),
    ('audience_role', 'job_seeker', 'Job Seeker'),
    ('audience_role', 'organizer', 'Organizer'),
    ('audience_role', 'community_member', 'Community Member')
ON CONFLICT (kind, value) DO NOTHING;

-- Categories (signal domains)
INSERT INTO tags (kind, value, display_name) VALUES
    ('category', 'food_security', 'Food Security'),
    ('category', 'housing', 'Housing'),
    ('category', 'healthcare', 'Healthcare'),
    ('category', 'mental_health', 'Mental Health'),
    ('category', 'education', 'Education'),
    ('category', 'employment', 'Employment'),
    ('category', 'legal_aid', 'Legal Aid'),
    ('category', 'immigrant_services', 'Immigrant Services'),
    ('category', 'youth_services', 'Youth Services'),
    ('category', 'senior_services', 'Senior Services'),
    ('category', 'disability_services', 'Disability Services'),
    ('category', 'substance_abuse', 'Substance Abuse'),
    ('category', 'domestic_violence', 'Domestic Violence'),
    ('category', 'environmental', 'Environmental'),
    ('category', 'civic_engagement', 'Civic Engagement'),
    ('category', 'arts_culture', 'Arts & Culture'),
    ('category', 'transportation', 'Transportation'),
    ('category', 'community_safety', 'Community Safety'),
    ('category', 'financial_assistance', 'Financial Assistance'),
    ('category', 'childcare', 'Childcare')
ON CONFLICT (kind, value) DO NOTHING;

-- Signal domains
INSERT INTO tags (kind, value, display_name) VALUES
    ('signal_domain', 'human_services', 'Human Services'),
    ('signal_domain', 'civic_economic', 'Civic & Economic Action'),
    ('signal_domain', 'cultural_ecological', 'Cultural & Ecological'),
    ('signal_domain', 'community_safety', 'Community Safety')
ON CONFLICT (kind, value) DO NOTHING;
