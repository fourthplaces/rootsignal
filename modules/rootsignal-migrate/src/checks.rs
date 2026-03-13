use crate::Migration;

pub struct Warning {
    pub migration: &'static str,
    pub line: usize,
    pub message: String,
}

/// Lint SQL migrations for patterns that need human review.
pub fn check(migrations: &[Migration]) -> Vec<Warning> {
    let mut warnings = Vec::new();

    for m in migrations {
        let Some(sql) = m.sql_text() else { continue };

        for (i, line) in sql.lines().enumerate() {
            let trimmed = line.trim().to_uppercase();
            let line_num = i + 1;

            if trimmed.starts_with("--") {
                continue;
            }

            if trimmed.contains("DROP TABLE") && !trimmed.contains("IF EXISTS") {
                warnings.push(Warning {
                    migration: m.name,
                    line: line_num,
                    message: "DROP TABLE without IF EXISTS".into(),
                });
            }

            if trimmed.contains("DROP COLUMN") && !trimmed.contains("IF EXISTS") {
                warnings.push(Warning {
                    migration: m.name,
                    line: line_num,
                    message: "DROP COLUMN without IF EXISTS".into(),
                });
            }

            if trimmed.contains("TRUNCATE") {
                warnings.push(Warning {
                    migration: m.name,
                    line: line_num,
                    message: "TRUNCATE — all rows will be deleted".into(),
                });
            }

            if trimmed.contains("DELETE") && !trimmed.contains("WHERE") {
                warnings.push(Warning {
                    migration: m.name,
                    line: line_num,
                    message: "DELETE without WHERE clause".into(),
                });
            }

            if trimmed.contains("RENAME TO") {
                warnings.push(Warning {
                    migration: m.name,
                    line: line_num,
                    message: "RENAME — verify no code references old name".into(),
                });
            }

            // NOT NULL without DEFAULT on ALTER TABLE (risks failing on existing rows)
            if trimmed.contains("NOT NULL")
                && !trimmed.contains("DEFAULT")
                && trimmed.contains("ADD COLUMN")
            {
                warnings.push(Warning {
                    migration: m.name,
                    line: line_num,
                    message: "ADD COLUMN NOT NULL without DEFAULT — will fail if table has rows"
                        .into(),
                });
            }
        }
    }

    warnings
}
