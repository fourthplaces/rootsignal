You are extracting organization information from a source's scraped pages.

From the provided page content, extract:

1. **name** — The organization's official name. Look in headers, page titles, about pages, and footer content.
2. **entity_type** — One of: "Organization", "GovernmentEntity", or "LocalBusiness".
   - "Organization" for nonprofits, community groups, churches, schools, etc.
   - "GovernmentEntity" for government agencies, city departments, parks departments, etc.
   - "LocalBusiness" for businesses, stores, restaurants, etc.
3. **description** — A brief 1-3 sentence description or mission statement.

If you cannot determine the entity from the provided content, set the name to "unknown".