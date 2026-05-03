// urchin-vault: projection — write urchin events into the Obsidian vault.
// Writes ONLY inside URCHIN block markers; human content is never touched.

use anyhow::Result;
use chrono::{NaiveDate, TimeZone, Utc};
use std::collections::BTreeMap;
use std::path::Path;
use urchin_core::journal::Journal;

use crate::{contract, writer};

/// Write (or update) the URCHIN:DAILY block in `{vault_root}/daily/{date}.md`.
///
/// Groups events from `date` by source, formats them as a markdown bullet list,
/// and splices the result into the block marker region.
pub fn project_daily(journal: &Journal, vault_root: &Path, date: NaiveDate) -> Result<()> {
    let events = journal.read_all()?;

    // Day boundary in UTC (events stored in UTC).
    let day_start = Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap());
    let day_end   = Utc.from_utc_datetime(&date.and_hms_opt(23, 59, 59).unwrap());

    let day_events: Vec<_> = events
        .iter()
        .filter(|e| e.timestamp >= day_start && e.timestamp <= day_end)
        .collect();

    let content = if day_events.is_empty() {
        "_No urchin events recorded for this day._".to_string()
    } else {
        // Group by source, sort within each group by timestamp.
        let mut grouped: BTreeMap<&str, Vec<_>> = BTreeMap::new();
        for e in &day_events {
            grouped.entry(e.source.as_str()).or_default().push(*e);
        }
        for v in grouped.values_mut() {
            v.sort_by_key(|e| e.timestamp);
        }

        let total = day_events.len();
        let mut lines = vec![
            format!("**{} events** recorded on {}", total, date.format("%Y-%m-%d")),
            String::new(),
        ];

        for (source, evs) in &grouped {
            lines.push(format!("**{}** ({} events)", source, evs.len()));
            for e in evs.iter().take(20) {
                let ts = e.timestamp.format("%H:%M");
                let first_line = e.content.lines().next().unwrap_or("").trim();
                let truncated = if first_line.len() > 120 {
                    format!("{}…", &first_line[..120])
                } else {
                    first_line.to_string()
                };
                lines.push(format!("- `{}` {}", ts, truncated));
            }
            if evs.len() > 20 {
                lines.push(format!("- _{} more events not shown_", evs.len() - 20));
            }
            lines.push(String::new());
        }

        lines.join("\n")
    };

    let daily_dir = vault_root.join("daily");
    let note_path = daily_dir.join(format!("{}.md", date.format("%Y-%m-%d")));

    writer::upsert_block(
        &note_path,
        contract::DAILY_OPEN,
        contract::DAILY_CLOSE,
        &content,
    )?;

    tracing::debug!("[vault] projected {} events to {}", day_events.len(), note_path.display());
    Ok(())
}
