import { useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { AlertCircle, CheckCheck, FileText, LoaderCircle, Save, ShieldCheck, ArrowLeft } from 'lucide-react';

import { api } from '@/api/client';
import { cn } from '@/lib/utils';
import { useAuthStore } from '@/store/auth';
import { useTranslation } from '@/hooks/useTranslation';
import ContextSidebar from './ContextSidebar';

interface Props {
  projectId: string;
  documentId?: string;
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

export default function TranslationWorkbench({ projectId, documentId: propDocumentId }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const user = useAuthStore((s) => s.user);
  const preferredSource = user?.preferredSourceLanguage || 'en';
  const [activeUnitId, setActiveUnitId] = useState<string | null>(null);
  const [draftText, setDraftText] = useState('');

  const selectedDocId = propDocumentId;

  const { data: docsData, isLoading: isLoadingDocs } = useQuery({
    queryKey: ['documents', projectId],
    queryFn: () => api.get<{ items: DocumentItem[] }>(`/projects/${projectId}/documents`),
  });

  const documents = Array.isArray(docsData?.items) ? docsData.items : [];
  const selectedDocument = documents.find((doc) => doc.id === selectedDocId) ?? null;

  const { data: unitsData, isLoading: isLoadingUnits } = useQuery({
    queryKey: ['workbench-units', projectId, selectedDocId],
    enabled: Boolean(selectedDocId),
    queryFn: () => api.get<UnitsResponse>(`/projects/${projectId}/units?documentId=${selectedDocId}&page=1&pageSize=1000`),
  });

  const units = Array.isArray(unitsData?.items) ? unitsData.items : [];

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

  return (
    <div className="h-full flex overflow-hidden bg-slate-100">
      <main className="flex-1 min-w-0 flex bg-slate-50">
        
        <section className="w-80 shrink-0 border-r border-slate-200 bg-white flex flex-col">
          <div className="px-5 py-4 border-b border-slate-200 bg-slate-50">
            <Link
              to="/project/$projectId/documents"
              params={{ projectId }}
              className="inline-flex items-center text-sm font-medium text-blue-600 hover:text-blue-700 mb-3"
            >
              <ArrowLeft className="w-4 h-4 mr-1" />
              {t('workbench.backToFiles')}
            </Link>
            <div className="text-xs font-semibold uppercase tracking-[0.16em] text-slate-400">{t('workbench.currentFile')}</div>
            <div className="mt-2 text-base font-semibold text-slate-900 truncate">
              {selectedDocument ? selectedDocument.title || selectedDocument.path : (isLoadingDocs ? t('workbench.loadingUnits') : t('workbench.selectFile'))}
            </div>
          </div>

          <div className="flex-1 overflow-y-auto p-3 space-y-2">
            {isLoadingDocs || (selectedDocId && isLoadingUnits) ? (
              <LoadingState label={t('workbench.loadingUnits')} />
            ) : !selectedDocId ? (
              <EmptyState title={t('workbench.selectFile')} description={t('workbench.selectFileDesc')} />
            ) : units.length === 0 ? (
              <EmptyState title={t('workbench.noUnits')} description={t('workbench.noUnitsDesc')} />
            ) : (
              units.map((unit, index) => {
                const primarySource = unit.sources[preferredSource] || Object.values(unit.sources)[0] || '';
                return (
                  <button
                    key={unit.id}
                    type="button"
                    onClick={() => setActiveUnitId(unit.id)}
                    className={cn(
                      'w-full rounded-xl border p-4 text-left transition-all',
                      unit.id === activeUnitId
                        ? 'border-blue-500 bg-blue-50 shadow-sm'
                        : 'border-slate-200 bg-white hover:border-slate-300 hover:bg-slate-50',
                    )}
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div>
                        <div className="text-[11px] font-semibold uppercase tracking-[0.18em] text-slate-400">
                          {t('workbench.unit')} {index + 1}
                        </div>
                      </div>
                      <StatusBadge status={unit.status} t={t} />
                    </div>
                    <div className="mt-3 line-clamp-3 text-sm leading-6 text-slate-700 whitespace-pre-wrap">
                      {primarySource || t('workbench.noSource')}
                    </div>
                  </button>
                );
              })
            )}
          </div>
        </section>

        <section className="flex-1 min-w-0 flex flex-col">
          <div className="border-b border-slate-200 bg-white px-6 py-4 flex items-center justify-between">
            <div className="flex items-center gap-4">
              <span className="text-sm font-medium text-slate-500">
                {activeUnit ? activeUnit.key : t('workbench.selectUnit')}
              </span>
            </div>
            {activeUnit && <StatusBadge status={activeUnit.status} large t={t} />}
          </div>

          <div className="flex-1 overflow-y-auto p-6">
            {!selectedDocId ? (
              <EditorPlaceholder title={t('workbench.chooseFileFirst')} description={t('workbench.chooseFileFirstDesc')} />
            ) : !activeUnit ? (
              <EditorPlaceholder title={t('workbench.selectUnit')} description={t('workbench.chooseUnitDesc')} />
            ) : (
              <div className="mx-auto max-w-5xl space-y-6">
                
                <section className="rounded-2xl border border-slate-200 bg-white p-6 shadow-sm">
                  <div className="flex items-center gap-2 text-sm font-semibold text-slate-900 mb-4">
                    <FileText className="h-4 w-4 text-blue-600" />
                    {t('workbench.sourceText')}
                  </div>
                  <div className="grid gap-4 xl:grid-cols-2">
                    <SourceBlock
                      language={preferredSource}
                      text={activeUnit.sources[preferredSource] || Object.values(activeUnit.sources)[0] || ''}
                      emphasized
                      emptyText={t('workbench.noSourceAvailable')}
                    />
                    <div className="grid gap-4">
                      {Object.entries(activeUnit.sources)
                        .filter(([language]) => language !== preferredSource)
                        .map(([language, text]) => (
                          <SourceBlock key={language} language={language} text={text} emptyText={t('workbench.noSourceAvailable')} />
                        ))}
                    </div>
                  </div>
                </section>

                <section className="rounded-2xl border border-slate-200 bg-white p-6 shadow-sm">
                  <div className="flex items-center justify-between gap-4 mb-4">
                    <div>
                      <div className="text-sm font-semibold text-slate-900">{t('workbench.targetTranslation')}</div>
                    </div>
                    {isDirty && (
                      <div className="inline-flex items-center rounded-full bg-amber-100 px-3 py-1 text-sm font-medium text-amber-700">
                        <AlertCircle className="mr-2 h-4 w-4" />
                        {t('workbench.unsavedChanges')}
                      </div>
                    )}
                  </div>

                  <textarea
                    value={draftText}
                    disabled={!activeUnit.permissions?.canEdit || isBusy}
                    onChange={(event) => setDraftText(event.target.value)}
                    className={cn(
                      'min-h-[240px] w-full rounded-xl border px-5 py-4 text-base leading-7 outline-none transition-colors resize-y',
                      activeUnit.permissions?.canEdit
                        ? 'border-slate-300 bg-slate-50 focus:border-blue-500 focus:bg-white'
                        : 'cursor-not-allowed border-slate-200 bg-slate-100 text-slate-500',
                    )}
                    placeholder={activeUnit.permissions?.canEdit ? t('workbench.placeholder') : t('workbench.noPermission')}
                  />

                  <div className="mt-6 flex flex-wrap gap-3">
                    <ActionButton
                      onClick={handleSave}
                      disabled={!activeUnit.permissions?.canEdit || !isDirty || isBusy}
                      busy={saveMutation.isPending}
                      icon={<Save className="h-4 w-4" />}
                      label={t('workbench.saveTranslation')}
                      tone="primary"
                    />
                    <ActionButton
                      onClick={() => reviewMutation.mutate(activeUnit.id)}
                      disabled={!activeUnit.permissions?.canReview || activeUnit.status === 'reviewed' || activeUnit.status === 'approved' || isBusy}
                      busy={reviewMutation.isPending}
                      icon={<CheckCheck className="h-4 w-4" />}
                      label={t('workbench.markReviewed')}
                      tone="secondary"
                    />
                    <ActionButton
                      onClick={() => approveMutation.mutate(activeUnit.id)}
                      disabled={!activeUnit.permissions?.canApprove || activeUnit.status === 'approved' || isBusy}
                      busy={approveMutation.isPending}
                      icon={<ShieldCheck className="h-4 w-4" />}
                      label={t('workbench.approveUnit')}
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

function SourceBlock({ language, text, emphasized = false, emptyText }: { language: string; text: string; emphasized?: boolean; emptyText: string }) {
  return (
    <div className={cn('rounded-xl border p-4', emphasized ? 'border-blue-200 bg-blue-50/70' : 'border-slate-200 bg-slate-50')}>
      <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-slate-400">{language}</div>
      <div className="mt-3 whitespace-pre-wrap text-base leading-7 text-slate-800">{text || emptyText}</div>
    </div>
  );
}

function StatusBadge({ status, large = false, t }: { status: string; large?: boolean; t: (key: string) => string }) {
  const normalized = status || 'untranslated';
  
  const getStatusText = (s: string) => {
    switch (s) {
      case 'approved': return t('workbench.status.approved');
      case 'reviewed': return t('workbench.status.reviewed');
      case 'translated': return t('workbench.status.translated');
      default: return t('workbench.status.untranslated');
    }
  }

  return (
    <span
      className={cn(
        'inline-flex items-center rounded-md font-semibold uppercase tracking-[0.16em]',
        large ? 'px-3 py-1.5 text-xs' : 'px-2 py-1 text-[10px]',
        normalized === 'approved'
          ? 'bg-green-100 text-green-700'
          : normalized === 'reviewed'
            ? 'bg-blue-100 text-blue-700'
            : normalized === 'translated'
              ? 'bg-slate-200 text-slate-700'
              : 'bg-slate-100 text-slate-500',
      )}
    >
      {getStatusText(normalized)}
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
        'inline-flex min-w-[140px] items-center justify-center rounded-lg px-4 py-2.5 text-sm font-medium transition disabled:cursor-not-allowed disabled:opacity-50',
        tone === 'primary' && 'bg-blue-600 text-white hover:bg-blue-700 shadow-sm',
        tone === 'secondary' && 'bg-white border border-slate-300 text-slate-700 hover:bg-slate-50 shadow-sm',
        tone === 'success' && 'bg-green-600 text-white hover:bg-green-700 shadow-sm',
      )}
    >
      {busy ? <LoaderCircle className="mr-2 h-4 w-4 animate-spin" /> : <span className="mr-2">{icon}</span>}
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
    <div className="rounded-xl border border-dashed border-slate-200 bg-slate-50 px-4 py-6 text-center">
      <div className="text-sm font-semibold text-slate-700">{title}</div>
      <p className="mt-2 text-xs leading-6 text-slate-500">{description}</p>
    </div>
  );
}

function EditorPlaceholder({ title, description }: { title: string; description: string }) {
  return (
    <div className="flex h-full items-center justify-center">
      <div className="max-w-md rounded-2xl border border-dashed border-slate-300 bg-white px-8 py-10 text-center shadow-sm">
        <div className="text-lg font-semibold text-slate-900">{title}</div>
        <p className="mt-2 text-sm leading-6 text-slate-500">{description}</p>
      </div>
    </div>
  );
}
