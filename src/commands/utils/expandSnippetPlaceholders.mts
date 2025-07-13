import crypto from "crypto";

/**
 * Expands predefined placeholders in the input string with their corresponding dynamic values.
 *
 * This function searches the input string for placeholders formatted as `${PLACEHOLDER_NAME}`
 * and replaces them with runtime values such as the current date and time components,
 * random numbers, or a UUID. If a placeholder is not recognized, it remains unchanged.
 *
 * Supported placeholders include:
 * - `${CURRENT_YEAR}`: Full current year (e.g., 2025)
 * - `${CURRENT_YEAR_SHORT}`: Last two digits of the current year (e.g., "25")
 * - `${CURRENT_MONTH}`: Month as zero-padded number (01-12)
 * - `${CURRENT_MONTH_NAME}`: Full month name (e.g., "July")
 * - `${CURRENT_MONTH_NAME_SHORT}`: Short month name (e.g., "Jul")
 * - `${CURRENT_DATE}`: Day of the month, zero-padded (01-31)
 * - `${CURRENT_DAY_NAME}`: Full weekday name (e.g., "Saturday")
 * - `${CURRENT_DAY_NAME_SHORT}`: Short weekday name (e.g., "Sat")
 * - `${CURRENT_HOUR}`: Hour in 24-hour format, zero-padded (00-23)
 * - `${CURRENT_MINUTE}`: Minute, zero-padded (00-59)
 * - `${CURRENT_SECOND}`: Second, zero-padded (00-59)
 * - `${CURRENT_SECONDS_UNIX}`: Unix timestamp in seconds (number)
 * - `${CURRENT_TIMEZONE_OFFSET}`: Timezone offset from UTC in `±HH:mm` format (e.g., "+09:00")
 * - `${RANDOM}`: Random integer between 100000 and 999999 (inclusive)
 * - `${RANDOM_HEX}`: Random 6-digit hexadecimal string (e.g., "a1b2c3")
 * - `${UUID}`: Randomly generated UUID v4 string
 *
 * @param {string} input - The string containing placeholders to be expanded.
 * @returns {string} The input string with all recognized placeholders replaced by their values.
 *
 * @example
 * ```ts
 * const template = "Year: ${CURRENT_YEAR}, UUID: ${UUID}, Random: ${RANDOM}";
 * const result = expandSnippetPlaceholders(template);
 * console.log(result);
 * // Output: "Year: 2025, UUID: 550e8400-e29b-41d4-a716-446655440000, Random: 345678"
 * ```
 */
export function expandSnippetPlaceholders(input: string): string {
  const now = new Date();

  const pad = (num: number): string => String(num).padStart(2, "0");

  const replacements: Record<string, string | number> = {
    "${CURRENT_YEAR}": now.getFullYear(),
    "${CURRENT_YEAR_SHORT}": String(now.getFullYear()).slice(-2),
    "${CURRENT_MONTH}": pad(now.getMonth() + 1),
    "${CURRENT_MONTH_NAME}": now.toLocaleString("default", { month: "long" }),
    "${CURRENT_MONTH_NAME_SHORT}": now.toLocaleString("default", {
      month: "short",
    }),
    "${CURRENT_DATE}": pad(now.getDate()),
    "${CURRENT_DAY_NAME}": now.toLocaleString("default", { weekday: "long" }),
    "${CURRENT_DAY_NAME_SHORT}": now.toLocaleString("default", {
      weekday: "short",
    }),
    "${CURRENT_HOUR}": pad(now.getHours()),
    "${CURRENT_MINUTE}": pad(now.getMinutes()),
    "${CURRENT_SECOND}": pad(now.getSeconds()),
    "${CURRENT_SECONDS_UNIX}": Math.floor(now.getTime() / 1000),
    "${CURRENT_TIMEZONE_OFFSET}": (() => {
      const offset = -now.getTimezoneOffset();
      const sign = offset >= 0 ? "+" : "-";
      const hours = pad(Math.floor(Math.abs(offset) / 60));
      const minutes = pad(Math.abs(offset) % 60);
      return `${sign}${hours}:${minutes}`;
    })(),
    "${RANDOM}": Math.floor(100000 + Math.random() * 900000),
    "${RANDOM_HEX}": Math.floor(Math.random() * 0xffffff)
      .toString(16)
      .padStart(6, "0"),
    "${UUID}": crypto.randomUUID(),
  };

  return input.replace(/\$\{[A-Z_]+\}/g, (match) =>
    Object.prototype.hasOwnProperty.call(replacements, match)
      ? String(replacements[match])
      : match
  );
}

export default expandSnippetPlaceholders;
