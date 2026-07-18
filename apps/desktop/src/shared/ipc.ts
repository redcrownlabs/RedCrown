export const invoke = <T,>(method: string, params: Record<string, unknown> = {}) =>
  window.redcrown.invoke<T>(method, params);

export function messageOf(reason: unknown) {
  return reason instanceof Error ? reason.message : "Something went wrong";
}
