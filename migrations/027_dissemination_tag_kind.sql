-- Dissemination intent: how a listing should reach its audience.
-- Distinguishes passive searchable content from proactive broadcasts and urgent alerts.

INSERT INTO tag_kinds (slug, display_name, required, description, allowed_resource_types) VALUES
    ('dissemination', 'Dissemination', FALSE, 'How this signal should reach its audience', '{listing}')
ON CONFLICT (slug) DO NOTHING;

INSERT INTO tags (kind, value, display_name) VALUES
    ('dissemination', 'passive', 'Passive'),
    ('dissemination', 'announcement', 'Announcement'),
    ('dissemination', 'alert', 'Alert'),
    ('dissemination', 'advisory', 'Advisory')
ON CONFLICT (kind, value) DO NOTHING;
