// Number and message formatting for the pt-BR UI.

const LOCALE = 'pt-BR';
/** Decimal places shown by default. */
const DEFAULT_DIGITS = 2;
/** Decimal places of a percentage. */
const PERCENT_DIGITS = 1;
/** Shown for a value that does not exist. */
export const NO_VALUE = '—';

/** Message of anything thrown (an `ApiError`, an `Error` or a value). */
export const errorMessage = (thrown: unknown) => (thrown instanceof Error ? thrown.message : String(thrown));

/** A number in pt-BR notation, or {@link NO_VALUE} for `null` and for NaN
 *  (a gap, as raw series values from the core mark one). */
export const formatNumber = (value: number | null | undefined, digits = DEFAULT_DIGITS) =>
  value == null || Number.isNaN(value) ? NO_VALUE : value.toLocaleString(LOCALE, { maximumFractionDigits: digits });

/** A share in 0..1 as a percentage, or {@link NO_VALUE} for `null`. */
export const formatPercent = (share: number | null | undefined) =>
  share == null ? NO_VALUE : `${(share * 100).toLocaleString(LOCALE, { maximumFractionDigits: PERCENT_DIGITS })}%`;

/** Most decimals an interval is shown with while its ends still read the same. */
const MAX_INTERVAL_DIGITS = 6;

/** A value with its 95 % interval, "2,01 (1,98–2,04)"; just the value without one.
 *  A narrow interval gets more decimals until its ends differ, so it never
 *  reads "0,18 (0,18–0,18)". */
export function formatWithInterval(
  value: number | null | undefined,
  interval: { low: number; high: number } | null | undefined,
  digits = DEFAULT_DIGITS,
) {
  if (!interval) return formatNumber(value, digits);
  let shown = digits;
  while (shown < MAX_INTERVAL_DIGITS && formatNumber(interval.low, shown) === formatNumber(interval.high, shown)) shown++;
  return `${formatNumber(value, shown)} (${formatNumber(interval.low, shown)}–${formatNumber(interval.high, shown)})`;
}

/** An integer count with thousands separators. */
export const formatCount = (count: number) => count.toLocaleString(LOCALE);

/** An instant as `AAAA-MM-DD HH:mm` (UTC, the logger's clock). */
export const formatDateTime = (epochMs: number) => new Date(epochMs).toISOString().slice(0, 16).replace('T', ' ');
