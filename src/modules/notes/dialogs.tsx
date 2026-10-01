import { ui, uiError } from "../../i18n/ui";
// 通用对话框 + 加密分区交互对话框（spec: 加密分区设置与确认 / 修改与移除分区密码）。

import { FormEvent, ReactNode, useState } from "react";
import { api } from "./api";
import { useNotesStore } from "./store";

export function Modal({
  title,
  onClose,
  children,
  wide,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
}) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      onClick={onClose}
    >
      <div
        className={`rounded-lg bg-white p-5 shadow-xl dark:bg-neutral-800 ${wide ? "w-[34rem]" : "w-96"}`}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="mb-3 text-lg font-semibold">{title}</h3>
        {children}
      </div>
    </div>
  );
}

export function usePrompt() {
  const [state, setState] = useState<{
    title: string;
    label: string;
    initial?: string;
    resolve?: (value: string | null) => void;
  } | null>(null);

  const prompt = (title: string, label: string, initial = "") =>
    new Promise<string | null>((resolve) =>
      setState({ title, label, initial, resolve }),
    );

  const element = state ? (
    <Modal
      title={state.title}
      onClose={() => {
        state.resolve?.(null);
        setState(null);
      }}
    >
      <form
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          const input = (e.target as HTMLFormElement).elements.namedItem(
            "prompt-input",
          ) as HTMLInputElement;
          state.resolve?.(input.value);
          setState(null);
        }}
      >
        <label className="mb-2 block text-sm">{state.label}</label>
        <input
          id="prompt-input"
          name="prompt-input"
          defaultValue={state.initial}
          autoFocus
          className="mb-3 w-full rounded border px-2 py-1 dark:bg-neutral-900"
        />
        <div className="flex justify-end gap-2">
          <button
            type="button"
            className="rounded border px-3 py-1"
            onClick={() => {
              state.resolve?.(null);
              setState(null);
            }}
          >{ui("取消")}</button>
          <button type="submit" className="rounded bg-blue-600 px-3 py-1 text-white">{ui("确定")}</button>
        </div>
      </form>
    </Modal>
  ) : null;

  return { prompt, element };
}

export function ConfirmDialog({
  title,
  message,
  confirmText = ui("确定"),
  danger,
  onConfirm,
  onClose,
}: {
  title: string;
  message: ReactNode;
  confirmText?: string;
  danger?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  return (
    <Modal title={title} onClose={onClose}>
      <div className="mb-4 text-sm">{message}</div>
      <div className="flex justify-end gap-2">
        <button className="rounded border px-3 py-1" onClick={onClose}>{ui("取消")}</button>
        <button
          className={`rounded px-3 py-1 text-white ${danger ? "bg-red-600" : "bg-blue-600"}`}
          onClick={() => {
            onConfirm();
            onClose();
          }}
        >
          {confirmText}
        </button>
      </div>
    </Modal>
  );
}

/** 密码输入 + 可选生成器（spec: 内置密码生成器入口）。 */
function PasswordField({
  value,
  onChange,
  showGenerate,
}: {
  value: string;
  onChange: (v: string) => void;
  showGenerate?: boolean;
}) {
  const [error, setError] = useState<string | null>(null);
  return (
    <div>
      <div className="flex gap-2">
        <input
          type="password"
          value={value}
          autoFocus
          onChange={(e) => onChange(e.target.value)}
          className="w-full rounded border px-2 py-1 dark:bg-neutral-900"
          placeholder={ui("输入分区密码")}
        />
        {showGenerate && (
          <button
            type="button"
            title={ui("使用密码生成器")}
            className="shrink-0 rounded border px-2 py-1 text-sm"
            onClick={async () => {
              try {
                onChange(await api.generatePassword(20));
                setError(null);
              } catch (e) {
                setError(uiError(e));
              }
            }}
          >{ui("🎲 生成")}</button>
        )}
      </div>
      {error && <p className="mt-1 text-sm text-red-600">{error}</p>}
    </div>
  );
}

function ErrorText({ error }: { error: string | null }) {
  if (!error) return null;
  return <p className="mt-2 text-sm text-red-600">{error}</p>;
}

/** 设置分区密码：强制确认「忘记密码不可恢复」+ 生成器入口。 */
export function SetPasswordDialog({
  spaceId,
  sectionId,
  onClose,
}: {
  spaceId: string;
  sectionId: string;
  onClose: () => void;
}) {
  const [password, setPassword] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const refreshTree = useNotesStore((s) => s.refreshTree);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!confirmed) {
      setError(ui("必须勾选确认「忘记密码则数据不可恢复」"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.setSectionPassword(spaceId, sectionId, password, confirmed);
      await refreshTree();
      onClose();
    } catch (err) {
      setError(uiError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal title={ui("设置分区密码")} onClose={onClose}>
      <form onSubmit={submit} className="space-y-3">
        <p className="rounded bg-amber-50 p-2 text-sm text-amber-800 dark:bg-amber-950 dark:text-amber-200">{ui("分区加密后忘记密码将无法恢复数据，请牢记密码或使用密码管理器保存。")}</p>
        <PasswordField value={password} onChange={setPassword} showGenerate />
        <label className="flex items-start gap-2 text-sm">
          <input
            type="checkbox"
            checked={confirmed}
            onChange={(e) => setConfirmed(e.target.checked)}
            className="mt-0.5"
          />
          <span>{ui("我已了解：忘记密码则该分区数据不可恢复")}</span>
        </label>
        <ErrorText error={error} />
        <div className="flex justify-end gap-2">
          <button type="button" className="rounded border px-3 py-1" onClick={onClose}>{ui("取消")}</button>
          <button
            type="submit"
            disabled={busy || !password}
            className="rounded bg-blue-600 px-3 py-1 text-white disabled:opacity-50"
          >
            {busy ? ui("加密中…") : ui("确认设置")}
          </button>
        </div>
      </form>
    </Modal>
  );
}

export function UnlockDialog({
  spaceId,
  sectionId,
  onClose,
}: {
  spaceId: string;
  sectionId: string;
  onClose: () => void;
}) {
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const refreshTree = useNotesStore((s) => s.refreshTree);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api.unlockSection(spaceId, sectionId, password);
      await refreshTree();
      onClose();
    } catch (err) {
      setError(uiError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal title={ui("解锁分区")} onClose={onClose}>
      <form onSubmit={submit} className="space-y-3">
        <PasswordField value={password} onChange={setPassword} />
        <ErrorText error={error} />
        <div className="flex justify-end gap-2">
          <button type="button" className="rounded border px-3 py-1" onClick={onClose}>{ui("取消")}</button>
          <button
            type="submit"
            disabled={busy || !password}
            className="rounded bg-blue-600 px-3 py-1 text-white disabled:opacity-50"
          >{ui("解锁")}</button>
        </div>
      </form>
    </Modal>
  );
}

/** 修改密码（需验证旧密码）/ 移除密码。 */
export function ChangePasswordDialog({
  spaceId,
  sectionId,
  mode,
  onClose,
}: {
  spaceId: string;
  sectionId: string;
  mode: "change" | "remove";
  onClose: () => void;
}) {
  const [oldPassword, setOldPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const refreshTree = useNotesStore((s) => s.refreshTree);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (mode === "change") {
        await api.changeSectionPassword(spaceId, sectionId, oldPassword, newPassword);
      } else {
        await api.removeSectionPassword(spaceId, sectionId, oldPassword);
      }
      await refreshTree();
      onClose();
    } catch (err) {
      setError(uiError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal title={mode === "change" ? ui("修改分区密码") : ui("移除分区密码")} onClose={onClose}>
      <form onSubmit={submit} className="space-y-3">
        {mode === "remove" && (
          <p className="rounded bg-amber-50 p-2 text-sm text-amber-800 dark:bg-amber-950 dark:text-amber-200">{ui("移除密码后分区数据将恢复为明文存储。")}</p>
        )}
        <PasswordField value={oldPassword} onChange={setOldPassword} />
        {mode === "change" && (
          <PasswordField value={newPassword} onChange={setNewPassword} showGenerate />
        )}
        <ErrorText error={error} />
        <div className="flex justify-end gap-2">
          <button type="button" className="rounded border px-3 py-1" onClick={onClose}>{ui("取消")}</button>
          <button
            type="submit"
            disabled={busy || !oldPassword || (mode === "change" && !newPassword)}
            className={`rounded px-3 py-1 text-white ${mode === "remove" ? "bg-red-600" : "bg-blue-600"}`}
          >
            {mode === "change" ? ui("确认修改") : ui("确认移除")}
          </button>
        </div>
      </form>
    </Modal>
  );
}
