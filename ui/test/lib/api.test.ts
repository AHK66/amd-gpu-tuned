import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ApiError,
  clearToken,
  deletePreset,
  getStatus,
  listPresets,
  lockNow,
  unlock,
} from "../../src/lib/api";
import { authState } from "../../src/lib/store/auth.svelte";
import { closeModal, isOpen } from "../../src/lib/store/modal.svelte";

function mockFetch(res: Response) {
  vi.stubGlobal("fetch", vi.fn(async () => res));
}

afterEach(() => vi.unstubAllGlobals());

describe("api req", () => {
  it("resolves null for a 204 No Content (empty body) response", async () => {
    mockFetch(new Response(null, { status: 204 }));
    await expect(deletePreset(5)).resolves.toBeNull();
  });

  it("parses JSON bodies on 2xx responses", async () => {
    mockFetch(
      new Response(JSON.stringify([{ id: 1, name: "a" }]), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    );
    await expect(listPresets()).resolves.toEqual([{ id: 1, name: "a" }]);
  });

  it("throws ApiError on non-ok responses", async () => {
    mockFetch(
      new Response(JSON.stringify({ error: "not found" }), {
        status: 404,
        headers: { "content-type": "application/json" },
      }),
    );
    await expect(deletePreset(99)).rejects.toBeInstanceOf(ApiError);
  });
});

describe("auth token handling", () => {
  beforeEach(() => {
    clearToken();
    closeModal();
    authState.unlocked = false;
  });

  function json(status: number, body: unknown) {
    return new Response(JSON.stringify(body), {
      status,
      headers: { "content-type": "application/json" },
    });
  }

  it("stores the token after unlock and attaches it to later requests", async () => {
    // Two params so `mock.calls.at(n)?.[1]` (the RequestInit) is well-typed.
    const fetchMock = vi.fn(async (url: string, _init?: RequestInit) =>
      url === "/api/auth/unlock"
        ? json(200, { token: "abc123", ttl_s: 1800 })
        : json(200, { ok: true }),
    );
    vi.stubGlobal("fetch", fetchMock);
    await unlock("pw");
    expect(authState.unlocked).toBe(true);
    await getStatus();
    const init = fetchMock.mock.calls.at(-1)?.[1] as RequestInit;
    expect((init.headers as Record<string, string>)["authorization"]).toBe(
      "Bearer abc123",
    );
  });

  it("omits the authorization header when no token is set", async () => {
    const fetchMock = vi.fn(async (_url?: unknown, _init?: RequestInit) =>
      json(200, { ok: true }),
    );
    vi.stubGlobal("fetch", fetchMock);
    await getStatus();
    const init = fetchMock.mock.calls.at(0)?.[1] as RequestInit;
    expect(
      (init.headers as Record<string, string> | undefined)?.["authorization"],
    ).toBeUndefined();
  });

  it("a 401 clears the token, flips unlocked, and opens the unlock modal", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url?: unknown, _init?: RequestInit) =>
        json(200, { token: "t1", ttl_s: 1800 }),
      ),
    );
    await unlock("pw");
    expect(authState.unlocked).toBe(true);

    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url?: unknown, _init?: RequestInit) =>
        json(401, { error: "authentication required" }),
      ),
    );
    await expect(getStatus()).rejects.toBeInstanceOf(ApiError);
    expect(authState.unlocked).toBe(false);
    expect(isOpen("unlock")).toBe(true);

    // The token is gone: the next request carries no authorization header.
    const fetchMock = vi.fn(async (_url?: unknown, _init?: RequestInit) =>
      json(200, { ok: true }),
    );
    vi.stubGlobal("fetch", fetchMock);
    await getStatus();
    const init = fetchMock.mock.calls.at(0)?.[1] as RequestInit;
    expect(
      (init.headers as Record<string, string> | undefined)?.["authorization"],
    ).toBeUndefined();
  });

  it("a failed unlock keeps unlocked false and leaves the modal open", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url?: unknown, _init?: RequestInit) =>
        json(401, { error: "wrong password" }),
      ),
    );
    await expect(unlock("wrong")).rejects.toBeInstanceOf(ApiError);
    expect(authState.unlocked).toBe(false);
    expect(isOpen("unlock")).toBe(true);
  });

  it("lockNow clears the token and flips unlocked", async () => {
    const fetchMock = vi.fn(async (url: string, _init?: RequestInit) =>
      url === "/api/auth/unlock"
        ? json(200, { token: "t1", ttl_s: 1800 })
        : json(200, { locked: true }),
    );
    vi.stubGlobal("fetch", fetchMock);
    await unlock("pw");
    await lockNow();
    expect(authState.unlocked).toBe(false);
    await getStatus();
    const init = fetchMock.mock.calls.at(-1)?.[1] as RequestInit;
    expect(
      (init.headers as Record<string, string> | undefined)?.["authorization"],
    ).toBeUndefined();
  });
});
