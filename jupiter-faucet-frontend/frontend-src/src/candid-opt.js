export function readOpt(value) {
  if (Array.isArray(value)) {
    return value.length > 0 ? value[0] : null;
  }
  return value === undefined ? null : value;
}
