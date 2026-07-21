import { describe, expect, it } from "vitest";

import {
  hasConfiguredCatalog,
  moveEndpoint,
  normalizeEndpoint,
  validateSource,
  validateTrackerList,
} from "./settings-model";

describe("source settings", () => {
  it("normalizes supported URLs", () => {
    expect(normalizeEndpoint("https://example.com/api")).toBe("https://example.com/api/");
  });

  it("rejects embedded credentials", () => {
    expect(() => normalizeEndpoint("https://user:secret@example.com")).toThrow("Credentials");
  });

  it("reorders fallbacks without mutation", () => {
    const endpoints = [
      { id: "a", url: "https://a.example/", enabled: true },
      { id: "b", url: "https://b.example/", enabled: true },
    ];
    const moved = moveEndpoint(endpoints, 1, -1);
    expect(moved.map((endpoint) => endpoint.id)).toEqual(["b", "a"]);
    expect(endpoints.map((endpoint) => endpoint.id)).toEqual(["a", "b"]);
  });

  it("requires an enabled fallback for an enabled source", () => {
    expect(
      validateSource({ id: "source", name: "Catalog", enabled: true, endpoints: [] }),
    ).toContain("fallback");
  });

  it("distinguishes a clean first run from a configured catalog", () => {
    const settings = {
      schema_version: 1,
      sources: [{ id: "primary", name: "Catalog", enabled: true, endpoints: [] }],
      stream_cache: {
        idle_expiration_secs: 3600,
        maximum_age_secs: 7200,
        size_budget_bytes: 1073741824,
      },
      tracker_list: {
        enabled: true,
        source: {
          kind: "url" as const,
          url: "https://example.com/trackers.txt",
        },
      },
      theme: "system" as const,
      hide_watched_movies: true,
    };

    expect(hasConfiguredCatalog(settings)).toBe(false);
    expect(hasConfiguredCatalog({
      ...settings,
      sources: [{
        ...settings.sources[0],
        endpoints: [{ id: "endpoint", url: "https://example.com/", enabled: true }],
      }],
    })).toBe(true);
  });

  it("accepts HTTPS tracker lists and absolute Windows paths", () => {
    expect(validateTrackerList({
      enabled: true,
      source: { kind: "url", url: "https://example.com/trackers.txt" },
    })).toBeUndefined();
    expect(validateTrackerList({
      enabled: true,
      source: { kind: "file", path: "C:\\trackers\\public.txt" },
    })).toBeUndefined();
  });

  it("rejects insecure tracker-list URLs and relative paths", () => {
    expect(validateTrackerList({
      enabled: true,
      source: { kind: "url", url: "http://example.com/trackers.txt" },
    })).toContain("HTTPS");
    expect(validateTrackerList({
      enabled: true,
      source: { kind: "file", path: "trackers.txt" },
    })).toContain("absolute");
  });
});
