export function parseJsonInput(value: string, invalidMessage: string) {
  try {
    return JSON.parse(value);
  } catch {
    throw new Error(invalidMessage);
  }
}

export function parseFunctionTestPayload(value: string, invalidMessage: string) {
  const parsed = parseJsonInput(value, invalidMessage);
  if (isRecord(parsed) && Object.hasOwn(parsed, "input")) {
    return {
      requestBody: parsed,
      input: parsed.input,
    };
  }
  return {
    requestBody: { input: parsed },
    input: parsed,
  };
}

export function safeBuildQueryString(value: string) {
  try {
    return buildQueryString(value, "");
  } catch {
    return "";
  }
}

export function buildQueryString(value: string, invalidMessage: string) {
  const parsed = parseJsonInput(value, invalidMessage);
  if (!isRecord(parsed)) {
    if (invalidMessage) throw new Error(invalidMessage);
    return "";
  }
  return new URLSearchParams(
    Object.entries(parsed).flatMap(([key, entry]) => {
      if (entry === null || typeof entry === "undefined") return [];
      if (Array.isArray(entry)) {
        return entry.map((item) => [key, String(item)] as [string, string]);
      }
      return [[key, String(entry)] as [string, string]];
    }),
  ).toString();
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
