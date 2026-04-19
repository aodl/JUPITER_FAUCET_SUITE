export function mergeRegisteredLandingData(current, { registered, registeredError } = {}) {
  const nextErrors = {
    ...(current?.errors || {}),
    registered: registeredError ?? null,
  };
  return {
    ...(current || {}),
    ...(registered === undefined ? {} : { registered }),
    hasAnyFailure: Object.values(nextErrors).some(Boolean),
    errors: nextErrors,
  };
}
