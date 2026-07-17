import type { AppSettings, SourceConfig, SourceEndpoint } from "./contract.generated";

export function hasConfiguredCatalog(settings: AppSettings): boolean {
  return settings.sources.some(
    (source) => source.enabled && source.endpoints.some((endpoint) => endpoint.enabled),
  );
}

export function normalizeEndpoint(value: string): string {
  const url = new URL(value.trim());
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error("Use an HTTP or HTTPS URL");
  }
  if (url.username || url.password) {
    throw new Error("Credentials are not allowed in API URLs");
  }
  url.hash = "";
  if (!url.pathname.endsWith("/")) url.pathname += "/";
  return url.toString();
}

export function moveEndpoint(
  endpoints: SourceEndpoint[],
  index: number,
  direction: -1 | 1,
): SourceEndpoint[] {
  const destination = index + direction;
  if (destination < 0 || destination >= endpoints.length) return endpoints;
  const next = [...endpoints];
  [next[index], next[destination]] = [next[destination], next[index]];
  return next;
}

export function validateSource(source: SourceConfig): string | undefined {
  if (!source.name.trim()) return "Source name is required";
  if (source.enabled && !source.endpoints.some((endpoint) => endpoint.enabled)) {
    return "Enable at least one fallback URL";
  }
  try {
    for (const endpoint of source.endpoints) normalizeEndpoint(endpoint.url);
  } catch (error) {
    return error instanceof Error ? error.message : "Invalid API URL";
  }
  return undefined;
}
