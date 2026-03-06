export type SseSend = (event: { event?: string; data: unknown }) => void;

export function makeSseStream(
  onOpen: (send: SseSend) => (() => void) | Promise<() => void>,
) {
  const encoder = new TextEncoder();

  return new ReadableStream<Uint8Array>({
    start(controller) {
      let closed = false;
      let cleanup: (() => void) | null = null;
      let pingTimer: ReturnType<typeof setInterval> | null = null;

      const send: SseSend = ({ event, data }) => {
        if (closed) return;
        const payload =
          (event ? `event: ${event}\n` : "") +
          `data: ${JSON.stringify(data)}\n\n`;
        try {
          controller.enqueue(encoder.encode(payload));
        } catch {
          // Client likely disconnected.
          closed = true;
          if (pingTimer) {
            clearInterval(pingTimer);
            pingTimer = null;
          }
          if (cleanup) cleanup();
          cleanup = null;
        }
      };

      const init = async () => {
        try {
          const maybeCleanup = await onOpen(send);
          cleanup = maybeCleanup;
        } catch (err) {
          send({ event: "error", data: { message: String(err) } });
          closed = true;
          controller.close();
          return;
        }
      };

      void init();

      pingTimer = setInterval(() => {
        send({ event: "ping", data: { ts: Date.now() } });
      }, 15_000);

      const doCleanup = () => {
        if (closed) return;
        closed = true;
        if (pingTimer) {
          clearInterval(pingTimer);
          pingTimer = null;
        }
        try {
          if (cleanup) cleanup();
        } finally {
          cleanup = null;
        }
      };

      // `cancel()` triggers for client disconnects.
      (controller as unknown as { __cleanup?: () => void }).__cleanup = doCleanup;
    },
    cancel() {
      const controller = this as unknown as { __cleanup?: () => void };
      controller.__cleanup?.();
    },
  });
}
