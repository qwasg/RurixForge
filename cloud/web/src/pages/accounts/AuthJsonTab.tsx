import { CircleCheck, FileJson, Upload, X } from 'lucide-react';
import { useRef, useState, type DragEvent, type FormEvent, type ReactNode } from 'react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Field, Input, Textarea } from '@/components/ui/form';
import { FormError } from '@/components/ui/misc';
import { parsePool, type PoolForm } from '@/lib/accounts';
import { adminApi } from '@/lib/api/admin';
import { errorMessage } from '@/lib/api/client';
import type { Account, Group, ImportCodexResult } from '@/lib/api/types';
import {
  buildImportCodexBody,
  MAX_AUTH_JSON_BYTES,
  nameFromFilename,
  validateAuthJson,
  type AuthJsonSource,
} from '@/lib/authJson';
import { formatBytes, readFileText } from '@/lib/browser';
import { cn } from '@/lib/utils';
import { defaultPool, PoolFields } from './shared';

interface FileEntry {
  id: number;
  filename: string;
  size: number;
  text: string;
  error: string | null;
}

let nextFileId = 1;

async function toEntry(file: File): Promise<FileEntry> {
  const base = { id: nextFileId++, filename: file.name, size: file.size };
  if (file.size > MAX_AUTH_JSON_BYTES) return { ...base, text: '', error: '文件过大（上限 1 MB）' };
  try {
    const text = await readFileText(file);
    return { ...base, text, error: validateAuthJson(text) };
  } catch {
    return { ...base, text: '', error: '读取文件失败' };
  }
}

function AccountLine({ account }: { account: Account }) {
  return (
    <li className="flex items-center gap-2">
      <span className="font-medium">{account.name}</span>
      <span className="text-muted-foreground">
        {[account.email, account.planType].filter(Boolean).join(' · ') || `ID ${account.id}`}
      </span>
    </li>
  );
}

export function AuthJsonTab({ groups, onImported, onDone }: { groups: Group[]; onImported: () => void; onDone: () => void }) {
  const [pasted, setPasted] = useState('');
  const [pastedName, setPastedName] = useState('');
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [pool, setPool] = useState<PoolForm>(() => defaultPool(3));
  const [error, setError] = useState<ReactNode>(null);
  const [busy, setBusy] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [result, setResult] = useState<{ res: ImportCodexResult; labels: string[] } | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  const addFiles = async (list: FileList | File[]) => {
    const arr = Array.from(list);
    if (arr.length === 0) return;
    const entries = await Promise.all(arr.map(toEntry));
    setFiles((prev) => [...prev, ...entries]);
  };

  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    setDragging(false);
    void addFiles(e.dataTransfer.files);
  };

  const onSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const sources: AuthJsonSource[] = [];
    const problems: string[] = [];
    if (pasted.trim()) {
      const err = validateAuthJson(pasted);
      if (err) problems.push(`粘贴内容：${err}`);
      sources.push({ label: '粘贴内容', name: pastedName, text: pasted });
    }
    for (const f of files) {
      if (f.error) problems.push(`${f.filename}：${f.error}`);
      sources.push({ label: f.filename, name: nameFromFilename(f.filename), text: f.text });
    }
    if (sources.length === 0) return setError('请粘贴 auth.json 内容，或选择一个或多个 auth.json 文件');
    if (problems.length > 0) {
      return setError(
        <ul className="list-inside list-disc">
          {problems.map((p) => (
            <li key={p}>{p}</li>
          ))}
        </ul>,
      );
    }
    const parsed = parsePool(pool);
    if (!parsed.ok) return setError(parsed.error);

    setError(null);
    setBusy(true);
    try {
      const res = await adminApi.accounts.importCodex(buildImportCodexBody(sources, parsed.value));
      setResult({ res, labels: sources.map((s) => s.label) });
      if ((res.created?.length ?? 0) + (res.updated?.length ?? 0) > 0) onImported();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const reset = () => {
    setResult(null);
    setPasted('');
    setPastedName('');
    setFiles([]);
    setError(null);
  };

  if (result) {
    const created = result.res.created ?? [];
    const updated = result.res.updated ?? [];
    const errors = result.res.errors ?? [];
    return (
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-2 text-sm">
          <CircleCheck className="size-4 text-success" />
          <span className="font-medium">导入完成</span>
          <Badge tone="success">{`新建 ${created.length} 个`}</Badge>
          <Badge tone="info">{`更新 ${updated.length} 个`}</Badge>
          <Badge tone={errors.length > 0 ? 'danger' : 'neutral'}>{`失败 ${errors.length} 个`}</Badge>
        </div>
        {created.length > 0 ? (
          <div className="rounded-md border p-3 text-sm">
            <p className="mb-1 text-xs text-muted-foreground">新建的账号</p>
            <ul className="flex flex-col gap-1">
              {created.map((a) => (
                <AccountLine key={a.id} account={a} />
              ))}
            </ul>
          </div>
        ) : null}
        {updated.length > 0 ? (
          <div className="rounded-md border p-3 text-sm">
            <p className="mb-1 text-xs text-muted-foreground">已存在、已更新凭据的账号（同一 ChatGPT 账号重复导入）</p>
            <ul className="flex flex-col gap-1">
              {updated.map((a) => (
                <AccountLine key={a.id} account={a} />
              ))}
            </ul>
          </div>
        ) : null}
        {errors.length > 0 ? (
          <div className="rounded-md border border-danger/30 bg-danger/5 p-3 text-sm">
            <p className="mb-1 text-xs text-danger">导入失败</p>
            <ul className="flex flex-col gap-1">
              {errors.map((er) => (
                <li key={er.index}>
                  <span className="font-medium">{result.labels[er.index] ?? `第 ${er.index + 1} 项`}</span>：{er.message}
                </li>
              ))}
            </ul>
          </div>
        ) : null}
        <div className="flex justify-end gap-2">
          <Button onClick={reset}>继续导入</Button>
          <Button variant="primary" onClick={onDone}>
            完成
          </Button>
        </div>
      </div>
    );
  }

  return (
    <form onSubmit={onSubmit} className="flex flex-col gap-4" noValidate>
      <Field
        label="auth.json 内容"
        hint={
          <>
            Codex CLI 登录后凭据保存在 <code className="font-mono">~/.codex/auth.json</code>（Windows：
            <code className="font-mono">%USERPROFILE%\.codex\auth.json</code>），整份粘贴即可。
          </>
        }
      >
        <Textarea
          value={pasted}
          onChange={(e) => setPasted(e.target.value)}
          placeholder={'{\n  "OPENAI_API_KEY": null,\n  "tokens": { "id_token": "…", "access_token": "…", "refresh_token": "…", "account_id": "…" }\n}'}
          className="min-h-32 font-mono text-xs"
          spellCheck={false}
        />
      </Field>
      <Field label="账号名称（可选）" hint="留空则由服务端按邮箱命名；只作用于粘贴的内容">
        <Input value={pastedName} onChange={(e) => setPastedName(e.target.value)} />
      </Field>

      <div>
        <div className="mb-1 text-xs font-medium text-foreground/80">或上传文件（可多选）</div>
        <div
          onDragOver={(e) => {
            e.preventDefault();
            setDragging(true);
          }}
          onDragLeave={() => setDragging(false)}
          onDrop={onDrop}
          className={cn(
            'flex flex-col items-center gap-2 rounded-md border border-dashed border-input px-4 py-4 text-center text-xs text-muted-foreground',
            dragging && 'border-info bg-info/5',
          )}
        >
          <div className="flex items-center gap-2">
            <Button size="sm" onClick={() => fileInput.current?.click()}>
              <Upload />
              选择 auth.json 文件
            </Button>
            <span>或拖放到这里</span>
          </div>
          <span>文件名（去掉 .json）会作为账号名称，默认文件名 auth.json 除外</span>
          <input
            ref={fileInput}
            type="file"
            multiple
            accept=".json,application/json"
            className="hidden"
            aria-label="上传 auth.json 文件"
            onChange={(e) => {
              const input = e.currentTarget;
              const list = input.files ? Array.from(input.files) : [];
              input.value = '';
              void addFiles(list);
            }}
          />
        </div>
        {files.length > 0 ? (
          <ul className="mt-2 flex flex-col gap-1">
            {files.map((f) => (
              <li key={f.id} className="flex items-center gap-2 rounded-md border px-2 py-1 text-xs">
                <FileJson className="size-3.5 shrink-0 text-muted-foreground" />
                <span className="font-medium">{f.filename}</span>
                <span className="text-muted-foreground">{formatBytes(f.size)}</span>
                {f.error ? <span className="text-danger">{f.error}</span> : <span className="text-success">格式正确</span>}
                <Button
                  size="icon-sm"
                  variant="ghost"
                  className="ml-auto"
                  aria-label={`移除 ${f.filename}`}
                  onClick={() => setFiles((prev) => prev.filter((x) => x.id !== f.id))}
                >
                  <X />
                </Button>
              </li>
            ))}
          </ul>
        ) : null}
      </div>

      <PoolFields value={pool} onChange={setPool} groups={groups} />
      <FormError>{error}</FormError>
      <div className="flex justify-end">
        <Button type="submit" variant="primary" loading={busy}>
          导入
        </Button>
      </div>
    </form>
  );
}
