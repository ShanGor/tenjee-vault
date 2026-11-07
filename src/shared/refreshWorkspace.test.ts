import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../modules/notes/api";
import { taskApi } from "../modules/tasks/api";
import { useNotesStore } from "../modules/notes/store";
import { useTaskStore } from "../modules/tasks/store";
import { refreshWorkspace } from "./refreshWorkspace";

vi.mock("../modules/notes/api", () => ({ api: { listSpaces: vi.fn(), getTree: vi.fn() } }));
vi.mock("../modules/tasks/api", () => ({ taskApi: { lists: vi.fn(), listView: vi.fn() } }));

beforeEach(() => {
  vi.resetAllMocks();
  useNotesStore.setState({ spaces: [], currentSpaceId: "space", tree: null, unlocked: [] });
  useTaskStore.setState({ lists: [], selectedListId: "list", tree: [] });
});

describe("workspace refresh after exchange", () => {
  it("reloads the existing space tree and task list even when selection has not changed", async () => {
    const spaces = [{ id: "space", name: "Received space" }] as Awaited<ReturnType<typeof api.listSpaces>>;
    const tree = { pages: [{ id: "received" }], unlocked_section_ids: ["unlocked"] } as unknown as Awaited<ReturnType<typeof api.getTree>>;
    vi.mocked(api.listSpaces).mockResolvedValue(spaces);
    vi.mocked(api.getTree).mockResolvedValue(tree);
    vi.mocked(taskApi.lists).mockResolvedValue([{ id: "list" }] as Awaited<ReturnType<typeof taskApi.lists>>);
    vi.mocked(taskApi.listView).mockResolvedValue([]);
    await refreshWorkspace();
    expect(api.getTree).toHaveBeenCalledWith("space");
    expect(useNotesStore.getState()).toMatchObject({ spaces, tree, unlocked: ["unlocked"] });
    expect(taskApi.listView).toHaveBeenCalledWith("list");
  });

  it("clears removed data when the received workspace has no spaces or task lists", async () => {
    vi.mocked(api.listSpaces).mockResolvedValue([]);
    vi.mocked(taskApi.lists).mockResolvedValue([]);
    await refreshWorkspace();
    expect(useNotesStore.getState()).toMatchObject({ currentSpaceId: null, tree: null, unlocked: [] });
    expect(useTaskStore.getState()).toMatchObject({ selectedListId: null, tree: [] });
  });

  it("refreshes tasks even if notes cannot be loaded and reports the error", async () => {
    vi.mocked(api.listSpaces).mockRejectedValue(new Error("unavailable"));
    vi.mocked(taskApi.lists).mockResolvedValue([]);
    await expect(refreshWorkspace()).rejects.toThrow("unavailable");
    expect(taskApi.lists).toHaveBeenCalled();
  });
});
