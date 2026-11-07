import { useNotesStore } from "../modules/notes/store";
import { useTaskStore } from "../modules/tasks/store";

// Module views reload on entry; these stores survive navigation to exchange.
export async function refreshWorkspace() {
  const results = await Promise.allSettled([
    (async () => {
      const notes = useNotesStore.getState();
      const spaces = await notes.refreshSpaces();
      const current = useNotesStore.getState().currentSpaceId;
      const target = spaces.find(space => space.id === current)?.id ?? spaces[0]?.id;
      if (target) await useNotesStore.getState().selectSpace(target);
      else useNotesStore.setState({ currentSpaceId: null, tree: null, unlocked: [], loading: false });
    })(),
    useTaskStore.getState().loadLists(),
  ]);
  const failed = results.find(result => result.status === "rejected");
  if (failed?.status === "rejected") throw failed.reason;
}
