import { create } from "zustand";
import { taskApi, TaskList, TaskNode } from "./api";

type State = {
  lists: TaskList[];
  selectedListId: string | null;
  tree: TaskNode[];
  selectedTaskId: string | null;
  selected: Set<string>;
  loadLists(): Promise<void>;
  selectList(id: string): Promise<void>;
  refresh(): Promise<void>;
  toggleSelected(id: string): void;
};

export const useTaskStore = create<State>((set, get) => ({
  lists: [], selectedListId: null, tree: [], selectedTaskId: null, selected: new Set(),
  async loadLists() {
    const lists = await taskApi.lists();
    const selectedListId = get().selectedListId && lists.some((item) => item.id === get().selectedListId)
      ? get().selectedListId : lists[0]?.id ?? null;
    set({ lists, selectedListId });
    if (selectedListId) set({ tree: await taskApi.listView(selectedListId) });
  },
  async selectList(id) {
    set({ selectedListId: id, selectedTaskId: null, selected: new Set(), tree: await taskApi.listView(id) });
  },
  async refresh() {
    const id = get().selectedListId;
    if (id) set({ tree: await taskApi.listView(id) });
  },
  toggleSelected(id) {
    const selected = new Set(get().selected);
    selected.has(id) ? selected.delete(id) : selected.add(id);
    set({ selected });
  },
}));
