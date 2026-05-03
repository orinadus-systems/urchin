// urchin-vault: contract — machine-owned block markers.
// All urchin-managed regions in vault files use these sentinels.
// Anything OUTSIDE these markers is never touched.

/// Daily activity block — one per daily note.
pub const DAILY_OPEN:  &str = "<!-- URCHIN:DAILY -->";
pub const DAILY_CLOSE: &str = "<!-- /URCHIN:DAILY -->";

/// Project context block — in each 10-projects/*.md file.
pub const PROJECT_OPEN:  &str = "<!-- URCHIN:PROJECT_CONTEXT -->";
pub const PROJECT_CLOSE: &str = "<!-- /URCHIN:PROJECT_CONTEXT -->";
