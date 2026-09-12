export interface ToolCallChunk {
  id?: string;
  name?: string;
  args?: string;
}

export interface StreamOptions {
  baseUrl: string;
  apiKey: string;
  model: string;
  messages: { role: string; content: string }[];
  temperature?: number;
  maxTokens?: number;
  tools?: unknown[];
  signal?: AbortSignal;
  onDelta?: (text: string) => void;
  onToolCallStart?: (calls: ToolCallChunk[]) => void;
}

export async function streamChat(opts: StreamOptions): Promise<void> {
  const url = `${opts.baseUrl.replace(/\/+$/, "")}/chat/completions`;
  const body: Record<string, unknown> = {
    model: opts.model,
    messages: opts.messages,
    stream: true,
  };
  if (opts.temperature !== undefined) body["temperature"] = opts.temperature;
  if (opts.maxTokens !== undefined) body["max_tokens"] = opts.maxTokens;
  if (opts.tools && opts.tools.length) body["tools"] = opts.tools;

  const res = await fetch(url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${opts.apiKey}`,
    },
    body: JSON.stringify(body),
    signal: opts.signal,
  });

  if (!res.ok) {
    const errText = await res.text().catch(() => "");
    throw new Error(`HTTP ${res.status}: ${errText.slice(0, 400)}`);
  }

  const reader = res.body?.getReader();
  if (!reader) throw new Error("无法读取响应流");

  const decoder = new TextDecoder();
  let buffer = "";
  const collectedToolCalls: Record<number, ToolCallChunk> = {};

  const flushTools = () => {
    const keys = Object.keys(collectedToolCalls).sort((a, b) => Number(a) - Number(b));
    if (keys.length) {
      const calls = keys
        .map((k) => collectedToolCalls[Number(k)])
        .filter((c): c is ToolCallChunk => c !== undefined);
      opts.onToolCallStart?.(calls);
    }
  };

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split("\n");
    buffer = lines.pop() ?? "";

    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed.startsWith("data:")) continue;
      const data = trimmed.slice(5).trim();
      if (data === "[DONE]") continue;
      let json: any;
      try {
        json = JSON.parse(data);
      } catch {
        continue;
      }
      const delta = json.choices?.[0]?.delta;
      if (!delta) continue;

      if (typeof delta.content === "string" && delta.content) {
        opts.onDelta?.(delta.content);
      }
      if (Array.isArray(delta.tool_calls)) {
        for (const tc of delta.tool_calls) {
          const idx: number = tc.index ?? 0;
          if (!collectedToolCalls[idx]) {
            collectedToolCalls[idx] = { id: tc.id, name: tc.function?.name || "", args: "" };
          } else {
            if (tc.id) collectedToolCalls[idx].id = tc.id;
            if (tc.function?.name) collectedToolCalls[idx].name = tc.function.name;
          }
          if (tc.function?.arguments) {
            collectedToolCalls[idx].args = (collectedToolCalls[idx].args || "") + tc.function.arguments;
          }
        }
      }
    }
  }

  flushTools();
}

export async function nonStreamChat(opts: Omit<StreamOptions, "onDelta" | "onToolCallStart">): Promise<string> {
  const url = `${opts.baseUrl.replace(/\/+$/, "")}/chat/completions`;
  const res = await fetch(url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${opts.apiKey}`,
    },
    body: JSON.stringify({
      model: opts.model,
      messages: opts.messages,
      ...(opts.temperature !== undefined ? { temperature: opts.temperature } : {}),
      ...(opts.maxTokens !== undefined ? { max_tokens: opts.maxTokens } : {}),
    }),
    signal: opts.signal,
  });
  if (!res.ok) {
    const errText = await res.text().catch(() => "");
    throw new Error(`HTTP ${res.status}: ${errText.slice(0, 400)}`);
  }
  const json = await res.json();
  return json.choices?.[0]?.message?.content ?? "";
}
