export type BridgeConfig = {
  baseUrl: string;
};

export function bridgeConfig(): BridgeConfig {
  const port = process.env.CODEX_WEB_BRIDGE_PORT ?? "4123";
  return { baseUrl: `http://127.0.0.1:${port}` };
}

export async function bridgeRpc(method: string, params: unknown) {
  const { baseUrl } = bridgeConfig();
  const res = await fetch(`${baseUrl}/rpc`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ method, params }),
    cache: "no-store",
  });
  const json = await res.json().catch(() => null);
  if (!res.ok) throw new Error(`bridge rpc failed: ${res.status} ${JSON.stringify(json)}`);
  return json;
}

export async function bridgeRespond(id: string, result: unknown) {
  const { baseUrl } = bridgeConfig();
  const res = await fetch(`${baseUrl}/respond`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id, result }),
    cache: "no-store",
  });
  const json = await res.json().catch(() => null);
  if (!res.ok) throw new Error(`bridge respond failed: ${res.status} ${JSON.stringify(json)}`);
  return json;
}

