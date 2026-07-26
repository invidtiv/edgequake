/**
 * 073 — Query Connections must show soft-labels, not bare UUID endpoints.
 *
 * Query UI streams via `/chat/completions/stream` (not `/query/stream`).
 * After stream, pending message is cleared and conversation detail is refetched —
 * so we mock both the SSE stream and GET /conversations/:id with soft-labels.
 */
import { expect, test } from "@playwright/test";
import { GOTO_OPTS } from "./helpers/navigation";
import { mockBackendForUiOnly } from "./helpers/mock-backend";

const CONV_ID = "spec073-conv";
const OPAQUE_ID = "84b69e27-e38b-444a-83dd-5e6a537c6f12";
const SOFT_SRC = "Future of work theme from the agenda";
const SOFT_TGT = "AI Next Conference";
/** UI truncates long soft-labels (formatEntityLabel maxLen=35). */
const SOFT_SRC_VISIBLE = /Future of work theme/;

function sseEvent(payload: Record<string, unknown>): string {
  return `data: ${JSON.stringify(payload)}\n\n`;
}

function mockOpaqueLabelStream(): string {
  return [
    sseEvent({
      type: "conversation",
      conversation_id: CONV_ID,
      user_message_id: "spec073-user",
    }),
    sseEvent({
      type: "context",
      sources: [
        {
          source_type: "chunk",
          id: "chunk-spec073",
          score: 0.91,
          snippet: "Conference agenda covers the future of work theme.",
          document_id: "doc-spec073",
        },
      ],
      subgraph: {
        entities: [
          {
            id: OPAQUE_ID,
            name: SOFT_SRC,
            entity_type: "CONCEPT",
            description: SOFT_SRC,
            score: 0.9,
            degree: 2,
          },
          {
            id: "AI_NEXT_CONFERENCE",
            name: "AI_NEXT_CONFERENCE",
            entity_type: "EVENT",
            description: "",
            score: 0.8,
            degree: 3,
          },
        ],
        relationships: [
          {
            id: `rel:${OPAQUE_ID}:HAS_THEME:AI_NEXT_CONFERENCE`,
            source: OPAQUE_ID,
            target: "AI_NEXT_CONFERENCE",
            source_label: SOFT_SRC,
            target_label: SOFT_TGT,
            relation_type: "HAS_THEME",
            description: "",
            score: 0.7,
          },
        ],
      },
      query_mode: "hybrid",
      retrieval_time_ms: 12,
    }),
    sseEvent({ type: "token", content: "Answer about the conference themes." }),
    sseEvent({
      type: "done",
      assistant_message_id: "spec073-assistant",
      tokens_used: 12,
      duration_ms: 40,
    }),
  ].join("");
}

const conversationDetail = {
  id: CONV_ID,
  tenant_id: "e2e-tenant-001",
  workspace_id: "e2e-ws-001",
  user_id: "e2e-user",
  title: "SPEC-073 conference themes",
  mode: "mix",
  is_pinned: false,
  is_archived: false,
  message_count: 2,
  meta: {},
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:01Z",
  messages: [
    {
      id: "spec073-user",
      conversation_id: CONV_ID,
      role: "user",
      content: "What themes does the conference cover?",
      is_error: false,
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    },
    {
      id: "spec073-assistant",
      conversation_id: CONV_ID,
      role: "assistant",
      content: "Answer about the conference themes.",
      is_error: false,
      created_at: "2026-01-01T00:00:01Z",
      updated_at: "2026-01-01T00:00:01Z",
      context: {
        sources: [
          {
            id: "chunk-spec073",
            content: "Conference agenda covers the future of work theme.",
            score: 0.91,
            source_type: "chunk",
            document_id: "doc-spec073",
          },
        ],
        entities: [
          {
            name: SOFT_SRC,
            entity_type: "CONCEPT",
            score: 0.9,
          },
        ],
        relationships: [
          {
            source: OPAQUE_ID,
            target: "AI_NEXT_CONFERENCE",
            source_label: SOFT_SRC,
            target_label: SOFT_TGT,
            relation_type: "HAS_THEME",
            score: 0.7,
          },
        ],
      },
    },
  ],
};

test.describe("073 query connections opaque labels", () => {
  test.setTimeout(90_000);

  test("Connections show soft-labels not raw UUID endpoints", async ({
    page,
  }) => {
    await mockBackendForUiOnly(page);

    // Folders + conversations must match API shapes or QueryInterface error-boundary remounts.
    await page.route("**/api/v1/folders**", async (route) => {
      if (route.request().method() !== "GET") {
        await route.fallback();
        return;
      }
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([]),
      });
    });

    await page.route("**/api/v1/conversations**", async (route) => {
      const url = route.request().url();
      const method = route.request().method();
      if (method === "GET" && url.includes(`/conversations/${CONV_ID}`)) {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify(conversationDetail),
        });
        return;
      }
      if (method === "GET") {
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            items: [],
            pagination: { has_more: false, next_cursor: null },
          }),
        });
        return;
      }
      await route.fallback();
    });

    await page.route("**/api/v1/chat/completions/stream", async (route) => {
      await route.fulfill({
        status: 200,
        headers: { "Content-Type": "text/event-stream" },
        body: mockOpaqueLabelStream(),
      });
    });

    await page.goto("/query", GOTO_OPTS);
    await page.waitForLoadState("domcontentloaded");
    await page.locator("main").first().waitFor({ state: "visible", timeout: 20_000 });

    await page
      .getByRole("textbox", { name: "Ask a question..." })
      .fill("What themes does the conference cover?");
    await page.getByRole("button", { name: /send/i }).click();

    await expect(
      page.getByText("Answer about the conference themes."),
    ).toBeVisible({ timeout: 30_000 });

    // Citations start collapsed; Connections live under Topics.
    await page.getByRole("button", { name: /Source citations:/i }).click();
    await page.getByRole("tab", { name: /Topics/i }).click();

    await expect(page.getByText("Connections")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByText(SOFT_SRC_VISIBLE).first()).toBeVisible();
    await expect(page.getByText(SOFT_TGT, { exact: false }).first()).toBeVisible();
    await expect(page.getByText(OPAQUE_ID, { exact: true })).toHaveCount(0);
  });
});
