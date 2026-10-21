// Commands built on the content module.

use crate::content::docx;
use crate::errors::AppError;

/// Export AI-generated meeting minutes as a Word document (.docx).
/// `title` (usually the recording's base name) becomes the document title;
/// `markdown` is the minutes body as produced by the LLM.
#[tauri::command]
pub fn export_meeting_minutes_docx(
    save_path: String,
    title: String,
    markdown: String,
) -> Result<(), AppError> {
    if markdown.trim().is_empty() {
        return Err(AppError::Validation {
            message: "minutes markdown must not be empty".to_string(),
            field: "markdown".to_string(),
        });
    }
    docx::write_minutes_docx(&save_path, &title, &markdown)
}
