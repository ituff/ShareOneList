/**
 * Validation utilities for file and folder names.
 */

/** Characters not allowed in file/folder names. */
const INVALID_CHARS = /[\\/:*?"<>|]/;

/** Maximum allowed file name length. */
const MAX_NAME_LENGTH = 400;

export interface ValidationResult {
  valid: boolean;
  error?: string;
}

/**
 * Validate a file or folder name against OneDrive/SharePoint naming rules.
 *
 * Rules:
 * - Must be between 1 and 400 characters
 * - Must not contain: \ / : * ? " < > |
 *
 * @example
 * validateFileName("")           // { valid: false, error: "File name cannot be empty" }
 * validateFileName("a".repeat(401)) // { valid: false, error: "File name must be 400 characters or fewer" }
 * validateFileName("file:name")  // { valid: false, error: "File name contains invalid character: :" }
 * validateFileName("valid.txt")  // { valid: true }
 */
export function validateFileName(name: string): ValidationResult {
  if (!name || name.length === 0) {
    return { valid: false, error: "File name cannot be empty" };
  }

  if (name.length > MAX_NAME_LENGTH) {
    return {
      valid: false,
      error: `File name must be ${MAX_NAME_LENGTH} characters or fewer`,
    };
  }

  const match = name.match(INVALID_CHARS);
  if (match) {
    return {
      valid: false,
      error: `File name contains invalid character: ${match[0]}`,
    };
  }

  return { valid: true };
}
