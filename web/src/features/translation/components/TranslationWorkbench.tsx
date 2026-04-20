import { useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { AlertCircle, CheckCheck, FileText, LoaderCircle, Save, ShieldCheck } from 'lucide-react';

import { api } from '@/api/client';
import { cn } from '@/lib/utils';
import { useAuthStore } from '@/store/auth';
import DocumentSidebar from '@/features/document/components/DocumentSidebar';
import ContextSidebar from './ContextSidebar';

interface Props {
  projectId: string;
}

interface DocumentTag {
  code: string;
  name: string;
  color: string;
}

interface DocumentItem {
  id: string;
  path: string;
  title: string;
  tags?: DocumentTag[];
}

interface UserSummary {
  id: string;
  name: string;
}

interface UnitPermissions {
  canView?: boolean;
  canEdit?: boolean;
  canReview?: boolean;
  canApprove?: boolean;
}

interface TranslationUnit {
  id: string;
  key: string;
  document: {
    id: string;
    path: string;
    tags?: DocumentTag[];
  };
  sources: Record<string, string>;
  target: {
    language: string;
    text: string;
  };
  status: string;
  comment: string;
  updatedAt: string;
  updatedBy?: UserSummary;
  permissions?: UnitPermissions;
}

interface UnitsResponse {
  items: TranslationUnit[];
  total: number;
}

export default function TranslationWorkbench({ projectId }: Props) {
  const queryClient = useQueryClient();
  const user = useAuthStore((s) => s.user);
  const preferredSource = user?.preferredSourceLanguage || 'en';
  const [selectedDocId, setSelectedDocId] = useState<string | null>(null);
  const [activeUnitId, setActiveUnitId] = useState<string | null>(null);
  const [draftText, setDraftText] = useState('');

  const { data: docsData, isLoading: isLoadingDocs } = useQuery({
    queryKey: ['documents', projectId],
    queryFn: () => api.get<{ items: DocumentItem[] }>(`/projects/${projectId}/documents`),
  });

  const documents = docsData?.items || [];

  useEffect(() => {
    if (!selectedDocId && documents.length > 0) {
      setSelectedDocId(documents[0].id);
    }
  }, [documents, selectedDocId]);

  const { data: unitsData, isLoading: isLoadingUnits } = useQuery({
    queryKey: ['workbench-units', projectId, selectedDocId],
    enabled: Boolean(selectedDocId),
    queryFn: () => api.get<UnitsResponse>(`/projects/${projectId}/units?documentId=${selectedDocId}&page=1&pageSize=1000`),
  });

  const units = unitsData?.items || [];

  useEffect(() => {
    if (!selectedDocId) {
      setActiveUnitId(null);
      return;
    }
    if (!units.length) {
      setActiveUnitId(null);
      return;
    }
    setActiveUnitId((current) => (current && units.some((unit) => unit.id === current) ? current : units[0].id));
  }, [selectedDocId, units]);

  const activeUnit = useMemo(
    () => units.find((unit) => unit.id === activeUnitId) ?? null,
    [activeUnitId, units],
  );

  useEffect(() => {
    setDraftText(activeUnit?.target?.text || '');
  }, [activeUnit]);

  const refreshUnits = () => {
    void queryClient.invalidateQueries({ queryKey: ['workbench-units', projectId, selectedDocId] });
  };

  const saveMutation = useMutation({
    mutationFn: (payload: { unitId: string; targetText: string }) =>
      api.patch<TranslationUnit>(`/projects/${projectId}/units/${payload.unitId}`, {
        targetText: payload.targetText,
        status: 'translated',
        comment: '',
      }),
    onSuccess: refreshUnits,
  });

  const reviewMutation = useMutation({
    mutationFn: (unitId: string) => api.post<TranslationUnit>(`/projects/${projectId}/units/${unitId}/review`, { status: 'reviewed', comment: '' }),
    onSuccess: refreshUnits,
  });

  const approveMutation = useMutation({
    mutationFn: (unitId: string) => api.post<TranslationUnit>(`/projects/${projectId}/units/${unitId}/approve`, { status: 'approved', comment: '' }),
    onSuccess: refreshUnits,
  });

  const isDirty = activeUnit ? draftText !== (activeUnit.target?.text || '') : false;
  const isBusy = saveMutation.isPending || reviewMutation.isPending || approveMutation.isPending;

  const handleSave = () => {
    if (!activeUnit || !activeUnit.permissions?.canEdit || !isDirty) {
      return;
    }
    saveMutation.mutate({ unitId: activeUnit.id, targetText: draftText });
  };

  const selectedDocument = documents.find((doc) => doc.id === selectedDocId) ?? null;

  return (
    <div className="h-full flex overflow-hidden bg-slate-100">
      <aside className="w-72 shrink-0 bg-white border-r border-slate-200 flex flex-col overflow-hidden">
        <DocumentSidebar
          documents={documents}
          selectedDocId={selectedDocId}
          onSelectDoc={(id) => {
            setSelectedDocId(id);
            setActiveUnitId(null);
          }}
        />
      </aside>

      <main className="flex-1 min-w-0 flex bg-slate-50">
        <section className="w-80 shrink-0 border-r border-slate-200 bg-white flex flex-col">
          <div className="px-5 py-4 border-b border-slate-200 bg-slate-50">
            <div className="text-xs font-semibold uppercase tracking-[0.16em] text-slate-400">Current file</div>
            <div className="mt-2 text-base font-semibold text-slate-900 truncate">
              {selectedDocument ? selectedDocument.title || selectedDocument.path : 'Select a file'}
            </div>
            <div className="mt-1 text-sm text-slate-500 truncate">{selectedDocument?.path || 'Choose a document to load its units.'}</div>
          </div>

          <div className="flex-1 overflow-y-auto p-3 space-y-2">
            {isLoadingDocs || (selectedDocId && isLoadingUnits) ? (
              <LoadingState label="Loading units..." />
            ) : !selectedDocId ? (
              <EmptyState title="Select a file" description="Pick a document from the left to see all units inside that file." />
            ) : units.length === 0 ? (
              <EmptyState title="No units in this file" description="This document currently has no translation units to edit." />
            ) : (
              units.map((unit, index) => {
                const primarySource = unit.sources[preferredSource] || Object.values(unit.sources)[0] || '';
                return (
                  <button
                    key={unit.id}
                    type="button"
                    onClick={() => setActiveUnitId(unit.id)}
                    className={cn(
                      'w-full rounded-2xl border p-4 text-left transition-all',
                      unit.id === activeUnitId
                        ? 'border-blue-500 bg-blue-50 shadow-sm'
                        : 'border-slate-200 bg-white hover:border-slate-300 hover:bg-slate-50',
                    )}
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div>
                        <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-slate-400">Unit {index + 1}</div>
                        <div className="mt-1 font-mono text-xs text-slate-500 break-all">{unit.key}</div>
                      </div>
                      <StatusBadge status={unit.status} />
                    </div>
                    <div className="mt-3 line-clamp-3 text-sm leading-6 text-slate-700 whitespace-pre-wrap">{primarySource || 'No source text'}</div>
                  </button>
                );
              })
            )}
          </div>
        </section>

        <section className="flex-1 min-w-0 flex flex-col">
          <div className="border-b border-slate-200 bg-white px-6 py-5">
            <div className="flex items-center justify-between gap-4">
              <div>
                <div className="text-xs font-semibold uppercase tracking-[0.16em] text-slate-400">Translation editor</div>
                <h2 className="mt-2 text-2xl font-semibold text-slate-900">{activeUnit?.key || 'Select a unit'}</h2>
                {activeUnit?.document?.path && <p className="mt-1 text-sm text-slate-500">{activeUnit.document.path}</p>}
              </div>
              {activeUnit && <StatusBadge status={activeUnit.status} large />}
            </div>
          </div>

          <div className="flex-1 overflow-y-auto p-6">
            {!selectedDocId ? (
              <EditorPlaceholder title="Choose a file first" description="After selecting a document, all units in that file will appear on the left and the active unit will open here." />
            ) : !activeUnit ? (
              <EditorPlaceholder title="Choose a unit" description="Select a unit from the left list to open the large translation editor." />
            ) : (
              <div className="mx-auto max-w-5xl space-y-6">
                <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
                  <div className="flex items-center gap-2 text-sm font-semibold text-slate-900">
                    <FileText className="h-4 w-4 text-blue-600" />
                    Source text
                  </div>
                  <div className="mt-4 grid gap-4 xl:grid-cols-2">
                    <SourceBlock
                      language={preferredSource}
                      text={activeUnit.sources[preferredSource] || Object.values(activeUnit.sources)[0] || ''}
                      emphasized
                    />
                    <div className="grid gap-4">
                      {Object.entries(activeUnit.sources)
                        .filter(([language]) => language !== preferredSource)
                        .map(([language, text]) => (
                          <SourceBlock key={language} language={language} text={text} />
                        ))}
                    </div>
                  </div>
                </section>

                <section className="rounded-3xl border border-slate-200 bg-white p-6 shadow-sm">
                  <div className="flex items-center justify-between gap-4">
                    <div>
                      <div className="text-sm font-semibold text-slate-900">Target translation</div>
                      <div className="mt-1 text-sm text-slate-500">Use the large editor below to update the active unit.</div>
                    </div>
                    {isDirty && (
                      <div className="inline-flex items-center rounded-full bg-amber-100 px-3 py-1 text-sm font-medium text-amber-700">
                        <AlertCircle className="mr-2 h-4 w-4" />
                        Unsaved changes
                      </div>
                    )}
                  </div>

                  <textarea
                    value={draftText}
                    disabled={!activeUnit.permissions?.canEdit || isBusy}
                    onChange={(event) => setDraftText(event.target.value)}
                    className={cn(
                      'mt-5 min-h-[320px] w-full rounded-3xl border px-5 py-4 text-base leading-7 outline-none transition-colors resize-y',
                      activeUnit.permissions?.canEdit
                        ? 'border-slate-300 bg-slate-50 focus:border-blue-500 focus:bg-white'
                        : 'cursor-not-allowed border-slate-200 bg-slate-100 text-slate-500',
                    )}
                    placeholder={activeUnit.permissions?.canEdit ? 'Write the translation for this unit here.' : 'You do not have permission to edit this unit.'}
                  />

                  <div className="mt-6 flex flex-wrap gap-3">
                    <ActionButton
                      onClick={handleSave}
                      disabled={!activeUnit.permissions?.canEdit || !isDirty || isBusy}
                      busy={saveMutation.isPending}
                      icon={<Save className="h-5 w-5" />}
                      label="Save translation"
                      tone="primary"
                    />
                    <ActionButton
                      onClick={() => reviewMutation.mutate(activeUnit.id)}
                      disabled={!activeUnit.permissions?.canReview || activeUnit.status === 'reviewed' || activeUnit.status === 'approved' || isBusy}
                      busy={reviewMutation.isPending}
                      icon={<CheckCheck className="h-5 w-5" />}
                      label="Mark reviewed"
                      tone="secondary"
                    />
                    <ActionButton
                      onClick={() => approveMutation.mutate(activeUnit.id)}
                      disabled={!activeUnit.permissions?.canApprove || activeUnit.status === 'approved' || isBusy}
                      busy={approveMutation.isPending}
                      icon={<ShieldCheck className="h-5 w-5" />}
                      label="Approve unit"
                      tone="success"
                    />
                  </div>
                </section>
              </div>
            )}
          </div>
        </section>

        <aside className="w-80 shrink-0 border-l border-slate-200 bg-slate-50">
          <ContextSidebar projectId={projectId} />
        </aside>
      </main>
    </div>
  );
}

function SourceBlock({ language, text, emphasized = false }: { language: string; text: string; emphasized?: boolean }) {
  return (
    <div className={cn('rounded-2xl border p-4', emphasized ? 'border-blue-200 bg-blue-50/70' : 'border-slate-200 bg-slate-50')}>
      <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-slate-400">{language}</div>
      <div className="mt-3 whitespace-pre-wrap text-base leading-7 text-slate-800">{text || 'No source text available.'}</div>
    </div>
  );
}

function StatusBadge({ status, large = false }: { status: string; large?: boolean }) {
  const normalized = status || 'untranslated';
  return (
    <span
      className={cn(
        'inline-flex items-center rounded-full font-semibold uppercase tracking-[0.16em]',
        large ? 'px-4 py-2 text-xs' : 'px-3 py-1 text-[11px]',
        normalized === 'approved'
          ? 'bg-green-100 text-green-700'
          : normalized === 'reviewed'
            ? 'bg-blue-100 text-blue-700'
            : normalized === 'translated'
              ? 'bg-slate-200 text-slate-700'
              : 'bg-slate-100 text-slate-500',
      )}
    >
      {normalized}
    </span>
  );
}

function ActionButton({
  onClick,
  disabled,
  busy,
  icon,
  label,
  tone,
}: {
  onClick: () => void;
  disabled: boolean;
  busy: boolean;
  icon: React.ReactNode;
  label: string;
  tone: 'primary' | 'secondary' | 'success';
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={cn(
        'inline-flex min-w-[180px] items-center justify-center rounded-2xl px-5 py-4 text-base font-semibold transition disabled:cursor-not-allowed disabled:opacity-50',
        tone === 'primary' && 'bg-blue-600 text-white hover:bg-blue-700',
        tone === 'secondary' && 'bg-slate-900 text-white hover:bg-slate-800',
        tone === 'success' && 'bg-green-600 text-white hover:bg-green-700',
      )}
    >
      {busy ? <LoaderCircle className="mr-2 h-5 w-5 animate-spin" /> : <span className="mr-2">{icon}</span>}
      {label}
    </button>
  );
}

function LoadingState({ label }: { label: string }) {
  return (
    <div className="flex h-full min-h-[160px] items-center justify-center text-sm text-slate-500">
      <LoaderCircle className="mr-2 h-4 w-4 animate-spin" />
      {label}
    </div>
  );
}

function EmptyState({ title, description }: { title: string; description: string }) {
  return (
    <div className="rounded-2xl border border-dashed border-slate-200 bg-slate-50 px-4 py-6 text-center">
      <div className="text-sm font-semibold text-slate-700">{title}</div>
      <p className="mt-2 text-sm leading-6 text-slate-500">{description}</p>
    </div>
  );
}

function EditorPlaceholder({ title, description }: { title: string; description: string }) {
  return (
    <div className="flex h-full items-center justify-center">
      <div className="max-w-lg rounded-3xl border border-dashed border-slate-300 bg-white px-8 py-10 text-center shadow-sm">
        <div className="text-xl font-semibold text-slate-900">{title}</div>
        <p className="mt-3 text-base leading-7 text-slate-500">{description}</p>
      </div>
    </div>
  );
}
