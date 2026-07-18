import { afterEach, describe, expect, it, vi } from "vitest";

import type { TorrentDiagnostics } from "../../shared/contract.generated";
import { startDiagnosticsPolling } from "./diagnostics-model";

afterEach(() => vi.useRealTimers());

describe("diagnostics polling", () => {
  it("continues polling after a transient IPC failure", async () => {
    vi.useFakeTimers();
    const diagnostics = { engine_state: "live" } as TorrentDiagnostics;
    const load = vi.fn<() => Promise<TorrentDiagnostics>>()
      .mockRejectedValueOnce(new Error("temporary failure"))
      .mockResolvedValue(diagnostics);
    const receive = vi.fn();
    const receiveError = vi.fn();
    const stop = startDiagnosticsPolling(
      load,
      receive,
      receiveError,
      (reason) => reason instanceof Error ? reason.message : "Unknown error",
    );

    await vi.advanceTimersByTimeAsync(0);
    expect(receiveError).toHaveBeenLastCalledWith("temporary failure");

    await vi.advanceTimersByTimeAsync(1000);
    expect(load).toHaveBeenCalledTimes(2);
    expect(receive).toHaveBeenLastCalledWith(diagnostics);
    expect(receiveError).toHaveBeenLastCalledWith(undefined);

    stop();
    await vi.advanceTimersByTimeAsync(2000);
    expect(load).toHaveBeenCalledTimes(2);
  });
});
