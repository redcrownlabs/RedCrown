import type { TorrentDiagnostics } from "../../shared/contract.generated";

type DiagnosticsLoader = () => Promise<TorrentDiagnostics>;
type DiagnosticsReceiver = (diagnostics: TorrentDiagnostics) => void;
type DiagnosticsErrorReceiver = (message?: string) => void;

/**
 * Polls diagnostics serially and recovers after transient IPC failures.
 *
 * Scheduling after completion prevents overlapping requests. An error remains
 * visible until the next successful response, but it does not freeze the live
 * diagnostics screen.
 */
export function startDiagnosticsPolling(
  load: DiagnosticsLoader,
  receive: DiagnosticsReceiver,
  receiveError: DiagnosticsErrorReceiver,
  messageOf: (reason: unknown) => string,
  intervalMilliseconds = 1000,
) {
  let active = true;
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function poll() {
    try {
      const diagnostics = await load();
      if (!active) return;
      receive(diagnostics);
      receiveError(undefined);
    } catch (reason) {
      if (!active) return;
      receiveError(messageOf(reason));
    } finally {
      if (active) timer = setTimeout(() => void poll(), intervalMilliseconds);
    }
  }

  void poll();
  return () => {
    active = false;
    if (timer) clearTimeout(timer);
  };
}
