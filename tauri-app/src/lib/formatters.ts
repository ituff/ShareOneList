/**
 * Utility formatters for human-readable display of file sizes and dates.
 */

const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;
const DIVISOR = 1024;

/**
 * Format a byte count into a human-readable file size string.
 * Uses 1024-based divisions with 1–2 decimal places.
 *
 * @example
 * formatFileSize(0)       // "0 B"
 * formatFileSize(512)     // "512 B"
 * formatFileSize(1536)    // "1.5 KB"
 * formatFileSize(1048576) // "1 MB"
 */
export function formatFileSize(bytes: number): string {
  if (bytes === 0) return "0 B";

  let unitIndex = 0;
  let size = bytes;

  while (size >= DIVISOR && unitIndex < UNITS.length - 1) {
    size /= DIVISOR;
    unitIndex++;
  }

  // Bytes are always displayed as integers
  if (unitIndex === 0) {
    return `${Math.round(size)} B`;
  }

  // For larger units, show 1-2 decimal places (trim trailing zero)
  const formatted = size.toFixed(2).replace(/\.?0+$/, "");
  // Ensure at least 1 decimal place for non-integer values
  if (!formatted.includes(".") && size % 1 !== 0) {
    return `${size.toFixed(1)} ${UNITS[unitIndex]}`;
  }

  return `${formatted} ${UNITS[unitIndex]}`;
}

/**
 * Format an ISO 8601 date string into a readable local date/time string.
 *
 * @example
 * formatDate("2024-01-15T10:30:00Z") // "2024/1/15 10:30"
 */
export function formatDate(isoString: string): string {
  const date = new Date(isoString);

  if (isNaN(date.getTime())) {
    return isoString;
  }

  const year = date.getFullYear();
  const month = date.getMonth() + 1;
  const day = date.getDate();
  const hours = date.getHours().toString().padStart(2, "0");
  const minutes = date.getMinutes().toString().padStart(2, "0");

  return `${year}/${month}/${day} ${hours}:${minutes}`;
}
