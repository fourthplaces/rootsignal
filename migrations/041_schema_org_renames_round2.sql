ALTER TABLE notes RENAME COLUMN created_by TO author;
ALTER TABLE schedules RENAME COLUMN freq TO repeat_frequency;
ALTER TABLE services RENAME COLUMN fees_description TO price_range;
ALTER TABLE services RENAME COLUMN eligibility_description TO eligibility;
