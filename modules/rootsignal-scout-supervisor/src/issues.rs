use neo4rs::query;
use tracing::info;

use rootsignal_graph::GraphClient;

use crate::types::ValidationIssue;

/// Manages ValidationIssue nodes in the graph.
pub struct IssueStore {
    client: GraphClient,
}

impl IssueStore {
    pub fn new(client: GraphClient) -> Self {
        Self { client }
    }

    /// Create a ValidationIssue node, but only if no open issue already exists
    /// for the same target_id and issue_type. Returns true if a new issue was created.
    pub async fn create_if_new(&self, issue: &ValidationIssue) -> Result<bool, neo4rs::Error> {
        // Check for existing open issue with same target + type
        let check = query(
            "MATCH (v:ValidationIssue {target_id: $target_id, issue_type: $issue_type})
             WHERE v.status = 'open'
             RETURN v.id AS id
             LIMIT 1"
        )
        .param("target_id", issue.target_id.to_string())
        .param("issue_type", issue.issue_type.to_string());

        let mut stream = self.client.inner().execute(check).await?;
        if let Some(_) = stream.next().await? {
            // Existing open issue — skip
            return Ok(false);
        }

        // Create new issue
        let ts = rootsignal_graph::writer::memgraph_datetime_pub(&issue.created_at);

        let create = query(
            "CREATE (v:ValidationIssue {
                id: $id,
                city: $city,
                issue_type: $issue_type,
                severity: $severity,
                target_id: $target_id,
                target_label: $target_label,
                description: $description,
                suggested_action: $suggested_action,
                status: $status,
                created_at: datetime($created_at)
            })"
        )
        .param("id", issue.id.to_string())
        .param("city", issue.city.clone())
        .param("issue_type", issue.issue_type.to_string())
        .param("severity", issue.severity.to_string())
        .param("target_id", issue.target_id.to_string())
        .param("target_label", issue.target_label.clone())
        .param("description", issue.description.clone())
        .param("suggested_action", issue.suggested_action.clone())
        .param("status", issue.status.to_string())
        .param("created_at", ts);

        self.client.inner().run(create).await?;
        Ok(true)
    }

    /// Auto-expire open issues older than 30 days.
    /// Returns the number of issues expired.
    pub async fn expire_stale_issues(&self) -> Result<u64, neo4rs::Error> {
        let q = query(
            "MATCH (v:ValidationIssue)
             WHERE v.status = 'open'
               AND v.created_at < datetime() - duration('P30D')
             SET v.status = 'resolved',
                 v.resolved_at = datetime(),
                 v.resolution = 'auto-expired after 30 days'
             RETURN count(v) AS expired"
        );

        let mut stream = self.client.inner().execute(q).await?;
        if let Some(row) = stream.next().await? {
            let expired: i64 = row.get("expired").unwrap_or(0);
            if expired > 0 {
                info!(expired, "Auto-expired stale ValidationIssue nodes");
            }
            return Ok(expired as u64);
        }
        Ok(0)
    }
}
