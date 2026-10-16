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

/** Allowed characters for a WebDAV gateway mount id (URL path segment). */
const MOUNT_ID_CHARS = /^[A-Za-z0-9_-]+$/;

/** Maximum allowed mount id length (matches the Rust-side limit). */
export const MAX_MOUNT_ID_LENGTH = 32;

/**
 * Validate a custom WebDAV gateway mount id (the `/m/{id}` URL segment).
 *
 * Rules:
 * - Must be between 1 and 32 characters
 * - Only ASCII letters, digits, `-` and `_` (percent-encoded segments
 *   confuse the Windows WebDAV redirector)
 */
export function validateMountId(id: string): ValidationResult {
  if (!id || id.length === 0) {
    return { valid: false, error: "Mount id cannot be empty" };
  }

  if (id.length > MAX_MOUNT_ID_LENGTH) {
    return {
      valid: false,
      error: `Mount id must be ${MAX_MOUNT_ID_LENGTH} characters or fewer`,
    };
  }

  if (!MOUNT_ID_CHARS.test(id)) {
    return {
      valid: false,
      error: "Mount id may only contain letters, digits, - and _",
    };
  }

  return { valid: true };
}
