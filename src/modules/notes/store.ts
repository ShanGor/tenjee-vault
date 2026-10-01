// zustand 全局状态：空间、导航树、解锁分区集合、设置（design D5）。

import { create } from "zustand";
import { api, AppSettings, SpaceInfo, TreeDto } from "./api";

interface NotesState {
  spaces: SpaceInfo[];
  tree: TreeDto | null;
  currentSpaceId: string | null;
  settings: AppSettings | null;
  unlocked: string[];
  loading: boolean;

  refreshSpaces: () => Promise<SpaceInfo[]>;
  selectSpace: (spaceId: string) => Promise<void>;
  refreshTree: () => Promise<void>;
  loadSettings: () => Promise<void>;
  setUnlocked: (ids: string[]) => void;
  removeUnlocked: (sectionId: string) => void;
  isSectionLocked: (section?: { is_encrypted: boolean; id: string } | null) => boolean;
}

export const useNotesStore = create<NotesState>((set, get) => ({
  spaces: [],
  tree: null,
  currentSpaceId: null,
  settings: null,
  unlocked: [],
  loading: false,

  refreshSpaces: async () => {
    const spaces = await api.listSpaces();
    set({ spaces });
    return spaces;
  },

  selectSpace: async (spaceId) => {
    set({ currentSpaceId: spaceId, tree: null });
    await get().refreshTree();
  },

  refreshTree: async () => {
    const spaceId = get().currentSpaceId;
    if (!spaceId) return;
    set({ loading: true });
    try {
      const tree = await api.getTree(spaceId);
      set({ tree, unlocked: tree.unlocked_section_ids });
    } finally {
      set({ loading: false });
    }
  },

  loadSettings: async () => {
    const settings = await api.getSettings();
    set({ settings });
  },

  setUnlocked: (ids) => set({ unlocked: ids }),

  removeUnlocked: (sectionId) =>
    set((s) => ({ unlocked: s.unlocked.filter((id) => id !== sectionId) })),

  // 加密且未解锁 = 锁定
  isSectionLocked: (section) =>
    !!section && section.is_encrypted && !get().unlocked.includes(section.id),
}));
