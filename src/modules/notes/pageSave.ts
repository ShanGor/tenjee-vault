// Flush the open document before navigation or changing its protection.
let saveCurrent: (() => Promise<void>) | null = null;
export function registerPageSave(save: () => Promise<void>): () => void {
  saveCurrent = save;
  return () => { if (saveCurrent === save) saveCurrent = null; };
}
export async function flushPageSave() { await saveCurrent?.(); }
