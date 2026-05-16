export const TABLE_MIN_PAGE_SIZE = 6;
export const TABLE_MAX_PAGE_SIZE = 18;
export const TABLE_ROW_ESTIMATE_PX = 48;
export const TABLE_VERTICAL_RESERVE_PX = 460;
export const COMMITMENT_TABLE_PAGE_SIZE_ADJUSTMENT = -1;

export function calculateResponsiveTablePageSize(viewportHeight = window.innerHeight) {
  const height = Number(viewportHeight);
  if (!Number.isFinite(height) || height <= 0) return TABLE_MIN_PAGE_SIZE;
  const available = Math.max(0, height - TABLE_VERTICAL_RESERVE_PX);
  const estimatedRows = Math.floor(available / TABLE_ROW_ESTIMATE_PX);
  return Math.min(TABLE_MAX_PAGE_SIZE, Math.max(TABLE_MIN_PAGE_SIZE, estimatedRows));
}
