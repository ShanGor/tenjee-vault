import { create } from "zustand";
import { taskApi, TaskList, TaskNode } from "./api";
import { readViewState, writeViewState } from "../../shared/viewState";

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

const savedSelection = readViewState<{ listId: string | null; taskId: string | null }>("tasks-selection", { listId: null, taskId: null });

export const useTaskStore = create<State>((set, get) => ({
  lists: [], selectedListId: savedSelection.listId, tree: [], selectedTaskId: savedSelection.taskId, selected: new Set(),
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

useTaskStore.subscribe((state) => {
  writeViewState("tasks-selection", { listId: state.selectedListId, taskId: state.selectedTaskId });
});
