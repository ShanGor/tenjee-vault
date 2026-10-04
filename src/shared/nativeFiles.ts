import { invoke } from "@tauri-apps/api/core";
import { open as desktopOpen, save as desktopSave, type OpenDialogOptions, type SaveDialogOptions } from "@tauri-apps/plugin-dialog";

export const isAndroid = () => /Android/i.test(navigator.userAgent) && "__TAURI_INTERNALS__" in window;

export async function open(options: OpenDialogOptions = {}): Promise<string | string[] | null> {
  try {
    if (!isAndroid()) return await desktopOpen(options);
    if (options.directory) return await invoke<string>("mobile_file_workspace_cmd");
    const paths = await invoke<string[]>("mobile_pick_files_cmd", { multiple: !!options.multiple, extensions: options.filters?.flatMap(f => f.extensions) ?? [] });
    return options.multiple ? paths : paths[0] ?? null;
  } catch (error) { window.dispatchEvent(new CustomEvent("native-file-error", {detail:error})); return null; }
}

export async function save(options: SaveDialogOptions = {}): Promise<string | null> {
  try {
    if (!isAndroid()) return await desktopSave(options);
    const directory = await invoke<string>("mobile_file_workspace_cmd");
    const name = (options.defaultPath ?? "export").split(/[\\/]/).pop()!.replace(/[\x00-\x1f]/g, "_");
    return `${directory}/${name === "." || name === ".." || !name ? "export" : name}`;
  } catch (error) { window.dispatchEvent(new CustomEvent("native-file-error", {detail:error})); return null; }
}

export async function finishExport(path: string, share = false): Promise<boolean> {
  if (!isAndroid()) return true;
  return invoke<boolean>("mobile_export_file_cmd", { path, share });
}

export async function finishTreeExport(directory: string, name: string): Promise<boolean> {
  if (!isAndroid()) return true;
  const path = await invoke<string>("mobile_zip_export_cmd", { directory: `${directory}/${name}` });
  try { return await finishExport(path); }
  finally { await releaseFiles([directory]); }
}

export async function releaseFiles(paths: string[]): Promise<void> {
  if (isAndroid()) await invoke("mobile_release_files_cmd", { paths });
}

export async function shareAttachment(data: { data_base64: string; file_name?: string; mime?: string | null }, name: string): Promise<void> {
  try { await invoke("mobile_share_bytes_cmd", { dataBase64: data.data_base64, filename: data.file_name ?? name }); }
  catch (error) { window.dispatchEvent(new CustomEvent("native-file-error", {detail:error})); }
}
