import assert from "node:assert/strict";
import test from "node:test";
import { PeanutClient, PeanutError } from "./index.js";

test("request sends app key and bearer token", async () => {
  const seenHeaders: Record<string, string | null> = {};
  const client = new PeanutClient({
    baseUrl: "https://peanut.test",
    appId: "app_1",
    apiKey: "pk_test",
    accessToken: "jwt_test",
    fetch: async (_url, init) => {
      const headers = new Headers(init?.headers);
      seenHeaders.apiKey = headers.get("x-peanut-api-key");
      seenHeaders.authorization = headers.get("authorization");
      return Response.json({ ok: true });
    },
  });

  await client.data.listTables();

  assert.equal(seenHeaders.apiKey, "pk_test");
  assert.equal(seenHeaders.authorization, "Bearer jwt_test");
});

test("request retries transient server failures", async () => {
  let attempts = 0;
  const client = new PeanutClient({
    baseUrl: "https://peanut.test",
    appId: "app_1",
    apiKey: "pk_test",
    retry: { maxRetries: 1, baseDelayMs: 1 },
    fetch: async () => {
      attempts += 1;
      if (attempts === 1) {
        return Response.json({ error: "temporary" }, { status: 503 });
      }
      return Response.json({ tables: [] });
    },
  });

  const response = await client.data.listTables();

  assert.deepEqual(response, { tables: [] });
  assert.equal(attempts, 2);
});

test("request does not retry client errors", async () => {
  let attempts = 0;
  const client = new PeanutClient({
    baseUrl: "https://peanut.test",
    appId: "app_1",
    apiKey: "pk_test",
    retry: { maxRetries: 2, baseDelayMs: 1 },
    fetch: async () => {
      attempts += 1;
      return Response.json({ error: "nope" }, { status: 403 });
    },
  });

  await assert.rejects(client.data.listTables(), PeanutError);
  assert.equal(attempts, 1);
});

test("data executeSql posts to app query endpoint", async () => {
  let seenUrl = "";
  let seenBody: unknown;
  const client = new PeanutClient({
    baseUrl: "https://peanut.test",
    appId: "app_1",
    apiKey: "pk_test",
    fetch: async (url, init) => {
      seenUrl = String(url);
      seenBody = JSON.parse(String(init?.body));
      return Response.json({
        statement: "select",
        table: "notes",
        columns: ["title"],
        rows: [{ title: "hello" }],
      });
    },
  });

  const result = await client.data.executeSql("select title from notes");

  assert.equal(seenUrl, "https://peanut.test/api/apps/app_1/data/query");
  assert.deepEqual(seenBody, { sql: "select title from notes" });
  assert.deepEqual(result.rows, [{ title: "hello" }]);
});
